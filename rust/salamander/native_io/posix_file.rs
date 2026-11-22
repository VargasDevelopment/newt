// SPDX-License-Identifier: MIT
//! Borrowed byte-path form of the existing OsNative open, without path backing.
use core::ffi::CStr;
use crate::{OsNative,Open,Result,open_mode,status,newt_io_open};

impl OsNative<'_>{
    pub(crate) fn open_cstr(&mut self,path:&CStr,mode:Open)->Result<i32>{
        let mut out=-1;
        // SAFETY: this native view holds the acquired UI domain. The exact
        // terminated byte path and exclusive fd output live through one open;
        // flags/mode are the existing shared mapping, with no retry or retention.
        status(unsafe{newt_io_open(path.as_ptr(),open_mode(mode),&mut out)})?;
        Ok(out)
    }
}
