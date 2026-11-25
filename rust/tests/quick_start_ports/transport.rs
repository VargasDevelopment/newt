// SPDX-License-Identifier: MIT
//! Synthetic NVS and the EXISTING FAT model; no production route is copied.
use crate::{home_startup::CardPort, model::Model, nvs};
use core::ffi::CStr;
use newt_native_io::UiIo;
use newt_native_storage::{Native, Open, Result, Stat};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
#[derive(Default)]
pub struct Flash {
    spaces: BTreeMap<String,BTreeMap<String,Vec<u8>>>,
    durable: BTreeMap<String,BTreeMap<String,Vec<u8>>>,
    handles: BTreeMap<u32,(String,bool)>,
    next: u32,
    pub writes: BTreeMap<String,u32>,
    pub fail: bool,
}
fn error() -> nvs::NativeError { nvs::NativeError::from_code(-1).unwrap_err() }
impl Flash {
    pub fn writes(&self,name:&str)->u32 { self.writes.get(name).copied().unwrap_or(0) }
}
impl nvs::Native for Flash {
    fn storage_ready(&mut self)->bool { true }
    fn open(&mut self,name:&CStr,readonly:bool,out:&mut u32)->core::result::Result<(),nvs::NativeError> {
        let name=name.to_str().unwrap().to_owned();
        if readonly && !self.spaces.contains_key(&name) {return Err(nvs::NativeError::NOT_FOUND);}
        self.spaces.entry(name.clone()).or_default();self.next+=1;*out=self.next;self.handles.insert(*out,(name,readonly));Ok(())
    }
    fn close(&mut self,handle:u32) { self.handles.remove(&handle); }
    fn get_blob(&mut self,handle:u32,key:&CStr,out:Option<&mut[u8]>,size:&mut usize)->core::result::Result<(),nvs::NativeError> {
        let name=&self.handles[&handle].0;
        let Some(bytes)=self.spaces[name].get(key.to_str().unwrap()) else{return Err(nvs::NativeError::NOT_FOUND)};
        if let Some(out)=out {if *size<bytes.len() || out.len()<bytes.len(){return Err(error());}out[..bytes.len()].copy_from_slice(bytes);}
        *size=bytes.len();Ok(())
    }
    fn set_blob(&mut self,handle:u32,key:&CStr,bytes:&[u8])->core::result::Result<(),nvs::NativeError> {
        let (name,readonly)=&self.handles[&handle];assert!(!readonly);*self.writes.entry(name.clone()).or_default()+=1;
        self.spaces.get_mut(name).unwrap().insert(key.to_str().unwrap().into(),bytes.to_vec());Ok(())
    }
    fn commit(&mut self,_:u32)->core::result::Result<(),nvs::NativeError> {
        if self.fail {Err(error())} else {self.durable=self.spaces.clone();Ok(())}
    }
    fn erase_key(&mut self,handle:u32,key:&CStr)->core::result::Result<(),nvs::NativeError> {
        let name=&self.handles[&handle].0;self.spaces.get_mut(name).unwrap().remove(key.to_str().unwrap());Ok(())
    }
    fn readback_durable(&self)->bool { self.spaces==self.durable }
}
#[derive(Clone)]
pub struct Card(pub Rc<RefCell<Model>>);
pub struct Port(Card);
impl Card {
    pub fn new()->Self {
        let mut model=Model::default();model.dirs.insert("/newt-sd".into());model.dirs.insert("/newt-sd/Newt".into());
        Self(Rc::new(RefCell::new(model)))
    }
    pub fn view(&self)->Port { Port(self.clone()) }
}
impl CardPort for Card {
    type Port<'ui>=Port;
    fn port<'ui>(&mut self,_:&'ui UiIo)->Port { self.view() }
}
impl Native for Port {
    fn open(&mut self,p:&str,m:Open)->Result<i32> {self.0.0.borrow_mut().open(p,m)}
    fn close(&mut self,fd:i32)->Result<()> {self.0.0.borrow_mut().close(fd)}
    fn read(&mut self,fd:i32,out:&mut[u8])->Result<usize> {self.0.0.borrow_mut().read(fd,out)}
    fn write(&mut self,fd:i32,bytes:&[u8])->Result<usize> {self.0.0.borrow_mut().write(fd,bytes)}
    fn seek(&mut self,fd:i32,at:u32)->Result<u64> {self.0.0.borrow_mut().seek(fd,at)}
    fn truncate(&mut self,fd:i32,n:u32)->Result<()> {self.0.0.borrow_mut().truncate(fd,n)}
    fn sync(&mut self,fd:i32)->Result<()> {self.0.0.borrow_mut().sync(fd)}
    fn fstat(&mut self,fd:i32)->Result<Stat> {self.0.0.borrow_mut().fstat(fd)}
    fn stat(&mut self,p:&str,n:bool)->Result<Stat> {self.0.0.borrow_mut().stat(p,n)}
    fn mkdir(&mut self,p:&str)->Result<()> {self.0.0.borrow_mut().mkdir(p)}
    fn unlink(&mut self,p:&str)->Result<()> {self.0.0.borrow_mut().unlink(p)}
    fn rename(&mut self,a:&str,b:&str)->Result<()> {self.0.0.borrow_mut().rename(a,b)}
    fn next_nonce(&mut self)->u32 {self.0.0.borrow_mut().next_nonce()}
}

impl nvs::Inventory for Flash {
    fn visit_entries(&mut self, visitor: &mut dyn FnMut(&[u8], &[u8], bool) -> bool)
        -> core::result::Result<(), nvs::NativeError> {
        for (name, values) in &self.spaces {
            for key in values.keys() {
                if !visitor(name.as_bytes(), key.as_bytes(), true) {
                    return nvs::NativeError::from_code(-1);
                }
            }
        }
        Ok(())
    }
}

impl newt_native_storage::Directory for Port {
    type Listing = u32;
    fn open_dir(&mut self, path: &str) -> Result<u32> { self.0.0.borrow_mut().open_dir(path) }
    fn read_dir(&mut self, listing: &mut u32, name: &mut [u8]) -> Result<Option<usize>> {
        self.0.0.borrow_mut().read_dir(listing, name)
    }
    fn close_dir(&mut self, listing: u32) -> Result<()> { self.0.0.borrow_mut().close_dir(listing) }
}
impl newt_native_storage::page_entries::PageEntries for Port {
    fn walk_entries(&mut self, folder: &CStr, sized: bool,
        visit: &mut dyn FnMut(&mut Self, newt_native_storage::page_entries::PageEntry<'_>) -> bool) -> bool {
        newt_native_storage::page_entries::walk_directory(self, folder, sized, visit)
    }
}
