// SPDX-License-Identifier: MIT
//! DEV cover observation's actual SDK scalars and borrowed directory names.
use core::ffi::CStr;
use crate::{UiIo,Result,status,WireStat};
use newt_native_storage::Stat;
pub use crate::posix_directory::Listing;
// SAFETY: linked SDK scalars are synchronous. Serial consumes the borrowed
// exact byte prefix once and retains no Rust pointer or independent buffer.
unsafe extern "C"{
    fn newt_cover_heap_monitor_start()->i32;
    fn newt_cover_heap_monitor_stop()->i32;
    fn newt_cover_heap_monitor_minimum()->usize;
    fn newt_cover_heap_monitor_largest()->usize;
    fn newt_cover_stack_min_free()->u32;
    fn nsc_write(bytes:*const u8,count:usize)->usize;
}
pub fn monitor_start()->i32{
    // SAFETY: cover's exclusive UI phase owns this one SDK local monitor lease.
    unsafe{newt_cover_heap_monitor_start()}
}
pub fn monitor_stop()->i32{
    // SAFETY: only the successful start's owner calls stop, exactly once.
    unsafe{newt_cover_heap_monitor_stop()}
}
pub fn monitor_minimum()->usize{
    // SAFETY: actual scalar read while this cover phase's SDK monitor is live.
    unsafe{newt_cover_heap_monitor_minimum()}
}
pub fn monitor_largest()->usize{
    // SAFETY: direct SDK largest block observation, no allocator-profile fallback.
    unsafe{newt_cover_heap_monitor_largest()}
}
pub fn stack_min_free()->u32{
    // SAFETY: actual current UI task high-watermark scalar; no pointer retained.
    unsafe{newt_cover_stack_min_free()}
}
pub fn write(_io:&UiIo,bytes:&[u8])->usize{
    // SAFETY: caller owns the live UI/Serial domain and has checked actual room
    // and pending input; this is one exact SDK write, without wake/retry/filter.
    unsafe{nsc_write(bytes.as_ptr(),bytes.len())}
}
pub fn stat_follow(_io:&UiIo,path:&CStr)->Result<Stat>{
    let mut out=WireStat::default();
    // SAFETY: existing one-call native stat with follow=true, complete borrowed
    // byte path and exact output. Census consumes only actual kind/size fields.
    status(unsafe{crate::newt_io_stat(path.as_ptr(),0,&mut out)})?;Ok(out.into())
}
