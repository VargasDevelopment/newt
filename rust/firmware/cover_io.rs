// SPDX-License-Identifier: MIT
//! Frozen CoverFiles/FaultLatch/FileSource on the existing native File calls.
use core::{cell::Cell, ffi::CStr};
use newt_native_io::{UiIo, cover::{self, Facts}};
use newt_native_storage::{Native, Error, Result};
use newt_core::zip_window::source::BookSource;
use super::observation::Observation;
pub trait Control {
    fn stopped(&self) -> bool;
    fn yield_now(&self);
    fn poll(&self) -> bool {
        if self.stopped() { return false; }
        self.yield_now(); !self.stopped()
    }
}
pub struct Files<'a> { io: &'a UiIo, faulted: Cell<bool>, observation:Option<&'a Observation> }
impl<'a> Files<'a> {
    pub fn new(io: &'a UiIo,observation:Option<&'a Observation>) -> Self {
        Self { io, faulted: Cell::new(false), observation }
    }
    fn note<T>(&self, value: Result<T>, absent: bool) -> Result<T> {
        if let Err(error) = value {
            if !absent || error != Error::Missing { self.faulted.set(true); }
        }
        value
    }
    pub fn faulted(&self) -> bool { self.faulted.get() }
    pub fn complete_handle_metadata(&self)->bool{cover::complete_handle_metadata()}
    pub fn rename_replaces_destination(&self)->bool{cover::rename_replaces_destination()}
    pub fn stat(&self, path: &CStr) -> Result<Facts> {
        if let Some(observation)=self.observation{observation.stat();}
        self.note(cover::stat(self.io,path),true)
    }
    pub fn fstat(&self, fd: i32) -> Result<Facts> { self.note(cover::fstat(self.io,fd),false) }
    pub fn open(&self, path: &CStr, write: bool) -> Result<i32> {
        if let Some(observation)=self.observation{observation.open();}
        self.note(cover::open(self.io,path,write),true)
    }
    pub fn read_at(&self, fd: i32, at: u64, out: &mut [u8]) -> Result<usize> {
        if let Some(observation)=self.observation{observation.read();}
        let mut native = self.io.port();
        let result = match u32::try_from(at) {
            Ok(at) => match native.seek(fd,at) {
                Ok(actual) if actual == u64::from(at) => native.read(fd,out),
                _ => Err(Error::Other),
            },
            Err(_) => Err(Error::Other),
        };
        if let Some(observation)=self.observation{observation.read_result(result.as_ref().ok().copied());}
        self.note(result,false)
    }
    pub fn write(&self, fd: i32, input: &[u8]) -> Result<usize> {
        if let Some(observation)=self.observation{observation.write();}
        let result = self.io.port().write(fd,input);
        if let Some(observation)=self.observation{observation.write_result(result.as_ref().ok().copied());}
        let result = self.note(result,false);
        if result.is_ok_and(|n| (n == 0 && !input.is_empty()) || n > input.len()) { self.faulted.set(true); }
        result
    }
    pub fn sync(&self, fd: i32) -> bool { self.note(self.io.port().sync(fd),false).is_ok() }
    pub fn close(&self, fd: i32) -> bool { self.note(self.io.port().close(fd),false).is_ok() }
    pub fn unlink(&self, path: &CStr) -> Result<()> {
        self.note(cover::unlink(self.io,path),true)
    }
    pub fn rename(&self, from: &CStr, to: &CStr) -> bool {
        self.note(cover::rename(self.io,from,to),false).is_ok()
    }
    pub fn mkdir(&self, path: &CStr) -> bool {
        self.note(cover::mkdir(self.io,path),false).is_ok()
    }
}
pub struct Source<'a> {
    files: &'a Files<'a>, control: &'a dyn Control, fd: i32,
    binding: Facts, faulted: Cell<bool>,
}
impl<'a> Source<'a> {
    pub fn new(files: &'a Files<'a>, control: &'a dyn Control) -> Self {
        Self { files, control, fd: -1, binding: Facts::default(), faulted: Cell::new(false) }
    }
    pub fn binding(&self) -> Facts { self.binding }
    pub fn bound(&self) -> bool { self.fd >= 0 }
    pub fn faulted(&self) -> bool { self.faulted.get() }
    pub fn open(&mut self, path: &CStr, maximum: u64) -> bool {
        self.close();
        if !self.control.poll() { return false; }
        let Ok(before) = self.files.stat(path) else { return false; };
        if !before.regular() || before.size <= 0 || before.seconds < 0 || before.size as u64 > maximum
            || !self.control.poll() { return false; }
        self.fd = self.files.open(path,false).unwrap_or(-1); self.binding = before;
        let ok = self.fd >= 0 && self.valid() && self.control.poll()
            && self.files.stat(path).is_ok_and(|s| self.binding.same(s));
        if !ok { self.close(); }
        ok
    }
    pub fn open_cache(&mut self, path: &CStr, maximum: u64) -> bool {
        self.close();
        if !self.control.poll() { return false; }
        self.fd = self.files.open(path,false).unwrap_or(-1);
        if self.fd < 0 { return false; }
        if self.control.poll() {
            if let Ok(binding) = self.files.fstat(self.fd) {
                self.binding = binding;
                if binding.regular() && binding.size > 0 && binding.seconds >= 0 && binding.size as u64 <= maximum { return true; }
            }
        }
        self.close(); false
    }
    pub fn valid(&self) -> bool {
        // Actual FAT fstat reports only size/mode. Full pathname facts are
        // checked at binding/publication, with complete transaction CRCs.
        if self.fd >= 0 && self.files.fstat(self.fd).is_ok_and(|s|
            if self.files.complete_handle_metadata(){self.binding.same(s)}
            else{s.regular() && s.size == self.binding.size}) { return true; }
        self.faulted.set(true); false
    }
    pub fn still_bound(&self, path: &CStr) -> bool {
        if !self.control.poll() || !self.valid() || !self.control.poll() { return false; }
        if self.files.stat(path).is_ok_and(|s| self.binding.same(s)) { return true; }
        self.faulted.set(true); false
    }
    pub fn close(&mut self) -> bool {
        let ok = self.fd < 0 || self.files.close(self.fd); self.fd = -1; ok
    }
}
impl BookSource for Source<'_> {
    fn size(&self) -> u64 { if self.fd < 0 { 0 } else { self.binding.size as u64 } }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if !self.control.poll() || self.fd < 0 || at > self.size() || out.len() as u64 > self.size()-at { return -1; }
        if !self.valid() { self.faulted.set(true); return -1; }
        let want = out.len().min(4096);
        let got = self.files.read_at(self.fd,at,&mut out[..want]);
        let Ok(got) = got else { self.faulted.set(true); return -1; };
        if (got == 0 && !out.is_empty()) || got > want || !self.valid() { self.faulted.set(true); return -1; }
        if self.control.poll() { got as i32 } else { -1 }
    }
}
impl Drop for Source<'_> { fn drop(&mut self) { self.close(); } }
pub fn write(files: &Files<'_>, control: &dyn Control, fd: i32, mut data: &[u8]) -> bool {
    while !data.is_empty() {
        if !control.poll() { return false; }
        let take = data.len().min(1024);
        match files.write(fd,&data[..take]) {
            Ok(got) if got != 0 && got <= take => data = &data[got..],
            _ => return false,
        }
    }
    true
}
pub fn remove_temporary(files: &Files<'_>, path: &CStr, control: Option<&dyn Control>) -> bool {
    if control.is_some_and(|c| !c.poll()) { return false; }
    match files.stat(path) {
        Err(Error::Missing) => true,
        Ok(info) => info.regular() && info.links <= 1 && !control.is_some_and(|c| !c.poll())
            && files.unlink(path).is_ok(),
        _ => false,
    }
}
pub fn source_crc(source: &mut Source<'_>, out: &mut u32) -> bool {
    let mut chunk = [0;1024]; let mut at = 0; *out = u32::MAX;
    while at < source.size() {
        let take = chunk.len().min((source.size()-at) as usize);
        let got = source.read_at(at,&mut chunk[..take]);
        if got <= 0 { return false; }
        *out = newt_core::cover::crc(*out,&chunk[..got as usize]); at += got as u64;
    }
    *out ^= u32::MAX; source.valid()
}
