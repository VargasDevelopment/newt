// SPDX-License-Identifier: MIT

//! Unsafe boundary for sdk_shim.cpp; keep outside newt-core (forbid unsafe).
use crate::nvs::{Native, NativeError};
use core::ffi::CStr;
use core::marker::PhantomData;
#[path="sdk/leaf.rs"]
mod leaf;
#[path="sdk/ports.rs"]
mod ports;
pub use ports::SdkPort;

/// No Copy, Clone, Send or Sync. Firmware owns the unique lease; this object
/// neither initializes NVS nor changes Newt's startup/flash-fence policy.
pub struct Sdk552(PhantomData<*mut ()>);
// Shared ports rely on this lease carrying no mutable Rust SDK state. The
// existing handles/flags remain in each backend's actual TransactionState.
const _:()=assert!(core::mem::size_of::<Sdk552>()==0);

impl Sdk552 {
    /// # Safety
    /// There must be no other independent live SDK context or backend for this
    /// integration. with_ports only borrows THIS lease; it creates no SDK owner.
    /// Link sdk_shim.cpp to the actual pinned IDF 5.5.2 and existing Newt guard
    /// wrappers. Their flash fence and initialization protection must stay active.
    /// Audit actual SDK/library provenance and synchronous readback contract;
    /// a version macro alone does not establish it. This packet has NOT done so.
    pub unsafe fn from_exclusive_firmware_lease() -> Self {
        Self(PhantomData)
    }
    /// The guard's raw NvsIssue for the boot notice (frozen reportNvsStorage).
    pub fn storage_issue(&self) -> u8 {
        leaf::storage_issue(self)
    }
}

impl Native for Sdk552 {
    fn storage_ready(&mut self) -> bool {
        leaf::storage_ready(self)
    }
    fn open(&mut self, name: &CStr, readonly: bool, handle: &mut u32) -> Result<(), NativeError> {
        leaf::open(self,name,readonly,handle)
    }
    fn close(&mut self, handle: u32) {
        leaf::close(self,handle);
    }
    fn get_blob(
        &mut self,
        handle: u32,
        key: &CStr,
        output: Option<&mut [u8]>,
        size: &mut usize,
    ) -> Result<(), NativeError> {
        leaf::get_blob(self,handle,key,output,size)
    }
    fn set_blob(&mut self, handle: u32, key: &CStr, bytes: &[u8]) -> Result<(), NativeError> {
        leaf::set_blob(self,handle,key,bytes)
    }
    fn commit(&mut self, handle: u32) -> Result<(), NativeError> {
        leaf::commit(self,handle)
    }
    fn erase_key(&mut self, handle: u32, key: &CStr) -> Result<(), NativeError> {
        leaf::erase_key(self,handle,key)
    }
    fn readback_durable(&self) -> bool {
        leaf::readback_durable(self)
    }
}

impl crate::nvs::Inventory for Sdk552 {
    fn visit_entries(&mut self, visitor: &mut dyn FnMut(&[u8], &[u8], bool) -> bool)
        -> Result<(), NativeError> {
        leaf::visit_entries(self, visitor)
    }
}
