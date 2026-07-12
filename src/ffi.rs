//! C ABI adapter for carrying `lazily-ipc` messages across FFI.
//!
//! The FFI surface deliberately owns only opaque channel handles and byte
//! buffers. Foreign runtimes exchange serialized [`IpcMessage`](crate::IpcMessage)
//! values without receiving Rust contexts, closures, references, or typed
//! handles.

use crate::ipc::IpcMessage;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::{ptr, slice};

/// Status code returned by FFI functions.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LazilyFfiStatus {
    /// Operation completed successfully.
    Ok = 0,
    /// The channel had no queued message.
    Empty = 1,
    /// A required pointer argument was null.
    NullPointer = 2,
    /// Input bytes were not a valid serialized `IpcMessage`.
    InvalidMessage = 3,
    /// An `IpcMessage` could not be encoded to the configured FFI codec.
    EncodeFailed = 4,
    /// A Rust panic was caught before crossing the ABI.
    Panic = 5,
}

/// Message variant observed in a serialized `IpcMessage`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LazilyFfiMessageKind {
    /// No message kind has been written.
    #[default]
    Unknown = 0,
    /// Full graph image.
    Snapshot = 1,
    /// Incremental graph update.
    Delta = 2,
    /// CRDT anti-entropy sync frame (multi-writer plane, `#lzcrdtplane5`).
    CrdtSync = 3,
}

/// Byte buffer allocated by Rust and returned across the FFI boundary.
///
/// Call [`lazily_ffi_bytes_free`] exactly once for each non-empty buffer
/// returned by this module.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LazilyFfiBytes {
    /// Pointer to the first byte, or null when `len == 0`.
    pub ptr: *mut u8,
    /// Number of initialized bytes at `ptr`.
    pub len: usize,
}

impl LazilyFfiBytes {
    /// Return an empty FFI buffer.
    pub const fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
        }
    }

    fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            return Self::empty();
        }

        let mut bytes = bytes.into_boxed_slice();
        let ptr = bytes.as_mut_ptr();
        let len = bytes.len();
        std::mem::forget(bytes);
        Self { ptr, len }
    }
}

impl Default for LazilyFfiBytes {
    fn default() -> Self {
        Self::empty()
    }
}

/// Opaque in-process FFI message channel.
///
/// The handle queues decoded [`IpcMessage`] values. Send functions validate the
/// caller's codec, and receive functions serialize into the requested codec.
/// It is a local ownership/ABI adapter, not a second graph state model.
#[derive(Debug, Default)]
pub struct LazilyFfiChannel {
    queue: VecDeque<IpcMessage>,
}

/// Create an empty FFI channel handle.
#[unsafe(no_mangle)]
pub extern "C" fn lazily_ffi_channel_new() -> *mut LazilyFfiChannel {
    Box::into_raw(Box::<LazilyFfiChannel>::default())
}

/// Free an FFI channel handle created by [`lazily_ffi_channel_new`].
///
/// # Safety
///
/// `channel` must be null or a pointer returned by `lazily_ffi_channel_new`
/// that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_channel_free(channel: *mut LazilyFfiChannel) {
    if channel.is_null() {
        return;
    }

    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Guaranteed by the caller's ownership contract above.
        unsafe { drop(Box::from_raw(channel)) };
    }));
}

/// Send one JSON-encoded `IpcMessage` through the FFI channel.
///
/// The input frame is decoded and re-encoded before enqueueing so receivers get
/// canonical bytes owned by Rust.
///
/// # Safety
///
/// `channel` must be a live pointer returned by `lazily_ffi_channel_new`.
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_channel_send_json(
    channel: *mut LazilyFfiChannel,
    ptr: *const u8,
    len: usize,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        // SAFETY: The public function safety contract covers both raw pointers.
        let channel = unsafe { channel_mut(channel) }?;
        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_json(bytes)?;
        channel.queue.push_back(message);
        Ok(())
    })
}

/// Receive one JSON-encoded `IpcMessage` from the FFI channel.
///
/// Writes an empty buffer and returns [`LazilyFfiStatus::Empty`] when no message
/// is currently queued.
///
/// # Safety
///
/// `channel` must be a live pointer returned by `lazily_ffi_channel_new`.
/// `out` must point to writable storage for one [`LazilyFfiBytes`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_channel_recv_json(
    channel: *mut LazilyFfiChannel,
    out: *mut LazilyFfiBytes,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        // SAFETY: The public function safety contract covers both raw pointers.
        let out = unsafe { out_bytes_mut(out) }?;
        *out = LazilyFfiBytes::empty();

        let channel = unsafe { channel_mut(channel) }?;
        match channel.queue.pop_front() {
            Some(message) => {
                *out = LazilyFfiBytes::from_vec(encode_message_json(&message)?);
                Ok(())
            }
            None => Err(LazilyFfiStatus::Empty),
        }
    })
}

/// Return the number of queued messages in the FFI channel.
///
/// # Safety
///
/// `channel` must be a live pointer returned by `lazily_ffi_channel_new`.
/// `out_len` must point to writable storage for one `usize`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_channel_len(
    channel: *const LazilyFfiChannel,
    out_len: *mut usize,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        // SAFETY: The public function safety contract covers both raw pointers.
        let channel = unsafe { channel_ref(channel) }?;
        let out_len = unsafe { out_len.as_mut() }.ok_or(LazilyFfiStatus::NullPointer)?;
        *out_len = channel.queue.len();
        Ok(())
    })
}

/// Validate that a byte slice is a JSON-encoded `IpcMessage`.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_validate_json(
    ptr: *const u8,
    len: usize,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        // SAFETY: The public function safety contract covers the raw byte range.
        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        decode_message_json(bytes)?;
        Ok(())
    })
}

/// Return the variant kind for a JSON-encoded `IpcMessage`.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`. `out_kind`
/// must point to writable storage for one [`LazilyFfiMessageKind`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_kind_json(
    ptr: *const u8,
    len: usize,
    out_kind: *mut LazilyFfiMessageKind,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        // SAFETY: The public function safety contract covers all raw pointers.
        let out_kind = unsafe { out_kind.as_mut() }.ok_or(LazilyFfiStatus::NullPointer)?;
        *out_kind = LazilyFfiMessageKind::Unknown;

        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_json(bytes)?;
        *out_kind = match message {
            IpcMessage::Snapshot(_) => LazilyFfiMessageKind::Snapshot,
            IpcMessage::Delta(_) => LazilyFfiMessageKind::Delta,
            IpcMessage::CrdtSync(_) => LazilyFfiMessageKind::CrdtSync,
        };
        Ok(())
    })
}

/// Validate and return canonical JSON bytes for a serialized `IpcMessage`.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`. `out` must
/// point to writable storage for one [`LazilyFfiBytes`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_clone_json(
    ptr: *const u8,
    len: usize,
    out: *mut LazilyFfiBytes,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        // SAFETY: The public function safety contract covers all raw pointers.
        let out = unsafe { out_bytes_mut(out) }?;
        *out = LazilyFfiBytes::empty();

        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_json(bytes)?;
        *out = LazilyFfiBytes::from_vec(encode_message_json(&message)?);
        Ok(())
    })
}

/// Send one binary-encoded `IpcMessage` through the FFI channel.
///
/// Requires the `ipc-binary` feature. Binary encoding uses `postcard` for
/// compact, non-self-describing frames.
///
/// # Safety
///
/// `channel` must be a live pointer returned by `lazily_ffi_channel_new`.
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`.
#[cfg(feature = "ipc-binary")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_channel_send_binary(
    channel: *mut LazilyFfiChannel,
    ptr: *const u8,
    len: usize,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let channel = unsafe { channel_mut(channel) }?;
        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_binary(bytes)?;
        channel.queue.push_back(message);
        Ok(())
    })
}

/// Receive one binary-encoded `IpcMessage` from the FFI channel.
///
/// Requires the `ipc-binary` feature.
///
/// # Safety
///
/// `channel` must be a live pointer returned by `lazily_ffi_channel_new`.
/// `out` must point to writable storage for one [`LazilyFfiBytes`].
#[cfg(feature = "ipc-binary")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_channel_recv_binary(
    channel: *mut LazilyFfiChannel,
    out: *mut LazilyFfiBytes,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let out = unsafe { out_bytes_mut(out) }?;
        *out = LazilyFfiBytes::empty();

        let channel = unsafe { channel_mut(channel) }?;
        match channel.queue.pop_front() {
            Some(message) => {
                *out = LazilyFfiBytes::from_vec(encode_message_binary(&message)?);
                Ok(())
            }
            None => Err(LazilyFfiStatus::Empty),
        }
    })
}

/// Validate that a byte slice is a binary-encoded `IpcMessage`.
///
/// Requires the `ipc-binary` feature.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`.
#[cfg(feature = "ipc-binary")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_validate_binary(
    ptr: *const u8,
    len: usize,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        decode_message_binary(bytes)?;
        Ok(())
    })
}

/// Return the variant kind for a binary-encoded `IpcMessage`.
///
/// Requires the `ipc-binary` feature.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`. `out_kind`
/// must point to writable storage for one [`LazilyFfiMessageKind`].
#[cfg(feature = "ipc-binary")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_kind_binary(
    ptr: *const u8,
    len: usize,
    out_kind: *mut LazilyFfiMessageKind,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let out_kind = unsafe { out_kind.as_mut() }.ok_or(LazilyFfiStatus::NullPointer)?;
        *out_kind = LazilyFfiMessageKind::Unknown;

        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_binary(bytes)?;
        *out_kind = match message {
            IpcMessage::Snapshot(_) => LazilyFfiMessageKind::Snapshot,
            IpcMessage::Delta(_) => LazilyFfiMessageKind::Delta,
            IpcMessage::CrdtSync(_) => LazilyFfiMessageKind::CrdtSync,
        };
        Ok(())
    })
}

/// Validate and return canonical binary bytes for a serialized `IpcMessage`.
///
/// Requires the `ipc-binary` feature.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`. `out` must
/// point to writable storage for one [`LazilyFfiBytes`].
#[cfg(feature = "ipc-binary")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_clone_binary(
    ptr: *const u8,
    len: usize,
    out: *mut LazilyFfiBytes,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let out = unsafe { out_bytes_mut(out) }?;
        *out = LazilyFfiBytes::empty();

        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_binary(bytes)?;
        *out = LazilyFfiBytes::from_vec(encode_message_binary(&message)?);
        Ok(())
    })
}

/// Send one MessagePack-encoded `IpcMessage` through the FFI channel.
///
/// Requires the `ipc-msgpack` feature. MessagePack encoding uses named fields
/// so frames stay cross-language and field-compatible with the JSON reference
/// codec (same field names; not byte-canonical across encoders).
///
/// # Safety
///
/// `channel` must be a live pointer returned by `lazily_ffi_channel_new`.
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`.
#[cfg(feature = "ipc-msgpack")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_channel_send_msgpack(
    channel: *mut LazilyFfiChannel,
    ptr: *const u8,
    len: usize,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let channel = unsafe { channel_mut(channel) }?;
        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_msgpack(bytes)?;
        channel.queue.push_back(message);
        Ok(())
    })
}

/// Receive one MessagePack-encoded `IpcMessage` from the FFI channel.
///
/// Requires the `ipc-msgpack` feature.
///
/// # Safety
///
/// `channel` must be a live pointer returned by `lazily_ffi_channel_new`.
/// `out` must point to writable storage for one [`LazilyFfiBytes`].
#[cfg(feature = "ipc-msgpack")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_channel_recv_msgpack(
    channel: *mut LazilyFfiChannel,
    out: *mut LazilyFfiBytes,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let out = unsafe { out_bytes_mut(out) }?;
        *out = LazilyFfiBytes::empty();

        let channel = unsafe { channel_mut(channel) }?;
        match channel.queue.pop_front() {
            Some(message) => {
                *out = LazilyFfiBytes::from_vec(encode_message_msgpack(&message)?);
                Ok(())
            }
            None => Err(LazilyFfiStatus::Empty),
        }
    })
}

/// Validate that a byte slice is a MessagePack-encoded `IpcMessage`.
///
/// Requires the `ipc-msgpack` feature.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`.
#[cfg(feature = "ipc-msgpack")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_validate_msgpack(
    ptr: *const u8,
    len: usize,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        decode_message_msgpack(bytes)?;
        Ok(())
    })
}

/// Return the variant kind for a MessagePack-encoded `IpcMessage`.
///
/// Requires the `ipc-msgpack` feature.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`. `out_kind`
/// must point to writable storage for one [`LazilyFfiMessageKind`].
#[cfg(feature = "ipc-msgpack")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_kind_msgpack(
    ptr: *const u8,
    len: usize,
    out_kind: *mut LazilyFfiMessageKind,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let out_kind = unsafe { out_kind.as_mut() }.ok_or(LazilyFfiStatus::NullPointer)?;
        *out_kind = LazilyFfiMessageKind::Unknown;

        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_msgpack(bytes)?;
        *out_kind = match message {
            IpcMessage::Snapshot(_) => LazilyFfiMessageKind::Snapshot,
            IpcMessage::Delta(_) => LazilyFfiMessageKind::Delta,
            IpcMessage::CrdtSync(_) => LazilyFfiMessageKind::CrdtSync,
        };
        Ok(())
    })
}

/// Validate and re-encode normalized MessagePack bytes for a serialized `IpcMessage`.
///
/// The output is the encoder's deterministic named-field encoding for this
/// message, but MessagePack is **not** byte-canonical across encoders (map key
/// order is encoder-defined) — do not treat these bytes as a cross-encoder
/// canonical form. `json` (or positional `postcard`) is the byte-canonical form;
/// `json` is the reference codec. Requires the `ipc-msgpack` feature.
///
/// # Safety
///
/// `ptr..ptr+len` must be readable for `len` bytes when `len > 0`. `out` must
/// point to writable storage for one [`LazilyFfiBytes`].
#[cfg(feature = "ipc-msgpack")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_ipc_message_clone_msgpack(
    ptr: *const u8,
    len: usize,
    out: *mut LazilyFfiBytes,
) -> LazilyFfiStatus {
    ffi_guard(|| {
        let out = unsafe { out_bytes_mut(out) }?;
        *out = LazilyFfiBytes::empty();

        let bytes = unsafe { bytes_from_raw(ptr, len) }?;
        let message = decode_message_msgpack(bytes)?;
        *out = LazilyFfiBytes::from_vec(encode_message_msgpack(&message)?);
        Ok(())
    })
}

/// Free an FFI byte buffer returned by this module.
///
/// # Safety
///
/// `bytes` must be empty or a buffer returned by this module that has not
/// already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lazily_ffi_bytes_free(bytes: LazilyFfiBytes) {
    if bytes.ptr.is_null() {
        return;
    }

    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Guaranteed by the caller's ownership contract above.
        unsafe {
            drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                bytes.ptr, bytes.len,
            )))
        };
    }));
}

fn ffi_guard(operation: impl FnOnce() -> Result<(), LazilyFfiStatus>) -> LazilyFfiStatus {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => LazilyFfiStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => LazilyFfiStatus::Panic,
    }
}

fn decode_message_json(bytes: &[u8]) -> Result<IpcMessage, LazilyFfiStatus> {
    IpcMessage::decode_json(bytes).map_err(|_| LazilyFfiStatus::InvalidMessage)
}

fn encode_message_json(message: &IpcMessage) -> Result<Vec<u8>, LazilyFfiStatus> {
    message
        .encode_json()
        .map_err(|_| LazilyFfiStatus::EncodeFailed)
}

#[cfg(feature = "ipc-binary")]
fn decode_message_binary(bytes: &[u8]) -> Result<IpcMessage, LazilyFfiStatus> {
    IpcMessage::decode_binary(bytes).map_err(|_| LazilyFfiStatus::InvalidMessage)
}

#[cfg(feature = "ipc-binary")]
fn encode_message_binary(message: &IpcMessage) -> Result<Vec<u8>, LazilyFfiStatus> {
    message
        .encode_binary()
        .map_err(|_| LazilyFfiStatus::EncodeFailed)
}

#[cfg(feature = "ipc-msgpack")]
fn decode_message_msgpack(bytes: &[u8]) -> Result<IpcMessage, LazilyFfiStatus> {
    IpcMessage::decode_msgpack(bytes).map_err(|_| LazilyFfiStatus::InvalidMessage)
}

#[cfg(feature = "ipc-msgpack")]
fn encode_message_msgpack(message: &IpcMessage) -> Result<Vec<u8>, LazilyFfiStatus> {
    message
        .encode_msgpack()
        .map_err(|_| LazilyFfiStatus::EncodeFailed)
}

unsafe fn channel_ref<'a>(
    channel: *const LazilyFfiChannel,
) -> Result<&'a LazilyFfiChannel, LazilyFfiStatus> {
    // SAFETY: The caller promises that `channel` is a live handle or null.
    unsafe { channel.as_ref() }.ok_or(LazilyFfiStatus::NullPointer)
}

unsafe fn channel_mut<'a>(
    channel: *mut LazilyFfiChannel,
) -> Result<&'a mut LazilyFfiChannel, LazilyFfiStatus> {
    // SAFETY: The caller promises that `channel` is a live handle or null.
    unsafe { channel.as_mut() }.ok_or(LazilyFfiStatus::NullPointer)
}

unsafe fn out_bytes_mut<'a>(
    out: *mut LazilyFfiBytes,
) -> Result<&'a mut LazilyFfiBytes, LazilyFfiStatus> {
    // SAFETY: The caller promises that `out` is writable or null.
    unsafe { out.as_mut() }.ok_or(LazilyFfiStatus::NullPointer)
}

unsafe fn bytes_from_raw<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], LazilyFfiStatus> {
    if ptr.is_null() {
        return if len == 0 {
            Ok(&[])
        } else {
            Err(LazilyFfiStatus::NullPointer)
        };
    }

    // SAFETY: The caller promises that the byte range is readable.
    Ok(unsafe { slice::from_raw_parts(ptr, len) })
}
