// SPDX-License-Identifier: MIT

//! Durable NVS blob backend. Initialization and guards stay in Salamander's
//! native layer; `reclaim` owns only transient boot migration allocations.
use core::ffi::CStr;
extern crate alloc;
#[path = "inventory.rs"]
mod inventory;
pub use inventory::Inventory;
#[path = "reclaim.rs"]
pub mod reclaim;
use newt_core::durable::{Backend, BlobRead, DurableWrite, StorageOpen};

/// Native esp_err_t, preserving every nonzero error without reclassification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeError(core::num::NonZeroI32);

impl NativeError {
    pub const NOT_FOUND: Self = Self(core::num::NonZeroI32::new(0x1102).unwrap());

    pub const fn from_code(code: i32) -> Result<(), Self> {
        match core::num::NonZeroI32::new(code) {
            Some(error) => Err(Self(error)),
            None => Ok(()),
        }
    }

    pub const fn code(self) -> i32 {
        self.0.get()
    }
}

/// A unique mutable native-call context, borrowed by one backend. None queries
/// size; Some lends a buffer only for this call. Implementations must respect its
/// length even on failure. They may change size or handle on a failed call.
pub trait Native {
    fn storage_ready(&mut self) -> bool;
    fn open(&mut self, name: &CStr, readonly: bool, handle: &mut u32) -> Result<(), NativeError>;
    fn close(&mut self, handle: u32);
    fn get_blob(
        &mut self,
        handle: u32,
        key: &CStr,
        output: Option<&mut [u8]>,
        size: &mut usize,
    ) -> Result<(), NativeError>;
    fn set_blob(&mut self, handle: u32, key: &CStr, bytes: &[u8]) -> Result<(), NativeError>;
    fn commit(&mut self, handle: u32) -> Result<(), NativeError>;
    fn erase_key(&mut self, handle: u32, key: &CStr) -> Result<(), NativeError>;
    /// No default: cached native implementations must explicitly return false.
    fn readback_durable(&self) -> bool;
}

// IDF 5.5.2 names contain 1..=15 bytes, followed by a NUL. Newt's
// supported callers use such names. Reject out-of-domain input before native IO.
struct Name([u8; 16]);
impl Name {
    fn new(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.is_empty() || bytes.len() > 15 || bytes.contains(&0) {
            return None;
        }
        let mut name = Self([0; 16]);
        name.0[..bytes.len()].copy_from_slice(bytes);
        Some(name)
    }

    fn as_c_str(&self) -> &CStr {
        // Construction supplies the terminator and excludes interior NULs.
        CStr::from_bytes_until_nul(&self.0).unwrap()
    }
}

/// Frozen NvsBlobBackend's handle/open state; no native context or handles escape.
#[repr(C)]
pub struct TransactionState {
    handle: u32,
    opened: bool,
}
impl TransactionState {
    const CLOSED: Self = Self { handle: 0, opened: false };
}
const _: () = assert!(core::mem::size_of::<TransactionState>() == 8);
const _: () = assert!(core::mem::align_of::<TransactionState>() == 4);

#[path = "operations.rs"]
mod operations;
use operations::Session;
use newt_native_storage::reading_state::ScopedTransaction;

/// Explicit begin/end lifecycle; no implicit close on Drop. Ordinary calls and
/// transaction loans use the same operation bodies and separate owned states.
pub struct NvsBlobBackend<'a, N: Native> {
    native: &'a mut N,
    state: TransactionState,
}
impl<'a, N: Native> NvsBlobBackend<'a, N> {
    pub fn new(native: &'a mut N) -> Self {
        Self { native, state: TransactionState::CLOSED }
    }
    fn with_session<R>(&mut self, work: impl FnOnce(&mut Session<'_, N>) -> R) -> R {
        work(&mut Session { native: &mut *self.native, state: &mut self.state })
    }
    fn set_and_commit(&mut self, key: &str, bytes: &[u8]) -> usize {
        self.with_session(|session| session.set_and_commit(key, bytes))
    }
    pub fn erase_key(&mut self, key: &str) -> bool {
        self.with_session(|session| session.erase_key(key))
    }
}
impl<N: Native> Backend for NvsBlobBackend<'_, N> {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        self.with_session(|session| session.begin(namespace, readonly))
    }
    fn end(&mut self) { self.with_session(|session| session.end()); }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        self.with_session(|session| session.read_blob(key, output))
    }
    fn put_bytes(&mut self, _permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.set_and_commit(key, bytes)
    }
    fn readback_durable(&self) -> bool { self.native.readback_durable() }
}
impl<N: Native> ScopedTransaction for NvsBlobBackend<'_, N> {
    type State = TransactionState;
    const CLOSED: TransactionState = TransactionState::CLOSED;
    fn with_transaction<R>(
        &mut self,
        state: &mut TransactionState,
        work: impl FnOnce(&mut dyn Backend) -> R,
    ) -> Option<R> {
        if self.state.opened { return None; }
        Some(work(&mut Session { native: &mut *self.native, state }))
    }
}

#[cfg(test)]
#[path = "../../tests/nvs_backend/fixtures.rs"]
pub mod fixtures;
