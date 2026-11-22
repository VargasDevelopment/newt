// SPDX-License-Identifier: MIT
//! Scoped native-call views of the SAME stateless SDK lease, never handles.
use super::{Sdk552,leaf};
use crate::nvs::{Native,NativeError};
use core::ffi::CStr;

/// No Clone/Copy or Send/Sync. The shared Sdk552 reference makes both ports
/// UI-only; its PhantomData excludes sharing across tasks. Each actual backend
/// separately owns its existing TransactionState and exclusively lent buffers.
pub struct SdkPort<'op> { sdk:&'op Sdk552 }
impl Sdk552 {
    /// Lend two native-call contexts without aliasing mutable Rust state.
    /// The exclusive outer borrow excludes every other use of this SDK lease;
    /// HRTB prevents either port or a backend borrowing it from escaping work.
    pub fn with_ports<R>(&mut self,
        work:impl for<'op> FnOnce(SdkPort<'op>,SdkPort<'op>)->R)->R {
        let sdk=&*self;
        work(SdkPort{sdk},SdkPort{sdk})
    }
}
impl Native for SdkPort<'_> {
    fn storage_ready(&mut self)->bool { leaf::storage_ready(self.sdk) }
    fn open(&mut self,name:&CStr,readonly:bool,handle:&mut u32)->Result<(),NativeError> {
        leaf::open(self.sdk,name,readonly,handle)
    }
    fn close(&mut self,handle:u32) { leaf::close(self.sdk,handle); }
    fn get_blob(&mut self,handle:u32,key:&CStr,output:Option<&mut[u8]>,size:&mut usize)->Result<(),NativeError> {
        leaf::get_blob(self.sdk,handle,key,output,size)
    }
    fn set_blob(&mut self,handle:u32,key:&CStr,bytes:&[u8])->Result<(),NativeError> {
        leaf::set_blob(self.sdk,handle,key,bytes)
    }
    fn commit(&mut self,handle:u32)->Result<(),NativeError> { leaf::commit(self.sdk,handle) }
    fn erase_key(&mut self,handle:u32,key:&CStr)->Result<(),NativeError> { leaf::erase_key(self.sdk,handle,key) }
    fn readback_durable(&self)->bool { leaf::readback_durable(self.sdk) }
}
