// SPDX-License-Identifier: MIT
//! Frozen wifi_network_device.h NvsWifiKeyBackend over the one existing NVS policy.
#![forbid(unsafe_code)]
use crate::nvs::{Native,NvsBlobBackend};
use newt_core::{durable::{Backend,BlobRead,DurableWrite,StorageOpen},
    wifi_network_store::workflow::{KeyBackend,KEY_NAMESPACE,KEY_NAME}};
const KEY_BYTES:usize=32;

/// Short update-boot owner. The only native handle/state is this actual
/// NvsBlobBackend's existing 8B transaction; readonly is the frozen key guard.
/// Explicit end keeps the existing backend lifecycle, including every failure.
pub struct NvsWifiKeyBackend<'a,N:Native> {
    backend:NvsBlobBackend<'a,N>,
    readonly:bool,
}
impl<'a,N:Native> NvsWifiKeyBackend<'a,N> {
    pub fn new(native:&'a mut N)->Self { Self{backend:NvsBlobBackend::new(native),readonly:true} }
}
impl<N:Native> Backend for NvsWifiKeyBackend<'_,N> {
    fn begin(&mut self,namespace:&str,readonly:bool)->StorageOpen {
        if namespace!=KEY_NAMESPACE { return StorageOpen::Failed; }
        self.readonly=readonly;
        self.backend.begin(KEY_NAMESPACE,readonly)
    }
    fn end(&mut self) { self.backend.end();self.readonly=true; }
    fn read_blob(&mut self,key:&str,output:&mut[u8])->BlobRead {
        if key!=KEY_NAME || output.len()!=KEY_BYTES { return BlobRead::Failed; }
        self.backend.read_blob(key,output)
    }
    // Core's ordinary readback calls THIS guarded read_blob; expected-byte
    // comparison stays in the existing durable workflow, as frozen.
    fn put_bytes(&mut self,permit:DurableWrite,key:&str,bytes:&[u8])->usize {
        if self.readonly || key!=KEY_NAME || bytes.len()!=KEY_BYTES { return 0; }
        self.backend.put_bytes(permit,key,bytes)
    }
    fn readback_durable(&self)->bool { self.backend.readback_durable() }
}
impl<N:Native> KeyBackend for NvsWifiKeyBackend<'_,N> {
    fn backend(&mut self)->&mut dyn Backend { self }
    fn erase_key(&mut self)->bool { !self.readonly && self.backend.erase_key(KEY_NAME) }
}
