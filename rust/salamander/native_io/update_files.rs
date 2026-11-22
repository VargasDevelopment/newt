// SPDX-License-Identifier: MIT
//! Frozen update_card.h transport. Core owns every fd, retry and image window.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use crate::{UiIo,OsNative,Open,Kind,Native};
use newt_core::update_card::{Files,FileInfo};

/// The existing stateless native view only. No descriptor, path, buffer or Board
/// reference survives any method; callers own explicit close even after errors.
pub struct UpdateFiles<'ui>{native:OsNative<'ui>}
impl<'ui> UpdateFiles<'ui>{
    pub fn new(io:&'ui UiIo)->Self{Self{native:io.port()}}
}
impl Files for UpdateFiles<'_>{
    fn open(&mut self,path:&CStr)->i32{
        self.native.open_cstr(path,Open::SourceReadOnly).unwrap_or(-1)
    }
    fn stat(&mut self,fd:i32)->Option<FileInfo>{
        self.native.fstat(fd).ok().map(|info|FileInfo{regular:info.kind==Kind::Regular,size:info.size})
    }
    fn read(&mut self,fd:i32,out:&mut[u8])->i32{
        match self.native.read(fd,out){Ok(bytes)=>i32::try_from(bytes).unwrap_or(-1),Err(_)=>-1}
    }
    fn seek(&mut self,fd:i32,at:u32)->i64{
        match self.native.seek(fd,at){Ok(actual)=>i64::try_from(actual).unwrap_or(-1),Err(_)=>-1}
    }
    fn close(&mut self,fd:i32){let _=self.native.close(fd);}
}
