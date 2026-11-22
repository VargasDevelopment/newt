// SPDX-License-Identifier: MIT
//! Frozen update-card directory scope, borrowing actual POSIX dirent names.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use crate::{UiIo,posix_directory::Listing};
use newt_core::update_card::Directory;

pub struct UpdateDirectory<'ui>{io:&'ui UiIo,listing:Option<Listing<'ui>>}
impl<'ui> UpdateDirectory<'ui>{
    pub fn new(io:&'ui UiIo)->Self{Self{io,listing:None}}
}
impl Directory for UpdateDirectory<'_>{
    fn open(&mut self,path:&CStr)->bool{
        self.close();
        self.listing=Listing::open(self.io,path).ok();
        self.listing.is_some()
    }
    fn next(&mut self)->Option<&CStr>{
        // The Core trait deliberately treats EOF and actual readdir failure
        // alike; both end this scan before its one explicit closedir.
        self.listing.as_mut()?.next().ok().flatten()
    }
    fn close(&mut self){if let Some(listing)=self.listing.take(){listing.close();}}
}
