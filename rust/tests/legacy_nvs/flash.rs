// SPDX-License-Identifier: MIT
//! Shared synthetic NVS capacity model beneath the production backend.
use crate::nvs;
use core::ffi::CStr;
use std::collections::BTreeMap;

#[derive(Clone, Default)]
pub struct Flash {
    pub spaces: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub handles: BTreeMap<u32, String>,
    pub non_blob: Option<(String, String)>,
    pub require_reclaim_before_write: bool,
    pub scans: u32,
    pub reads: u32,
    pub legacy_opens: u32,
    pub erased: u32,
    pub unavailable: bool,
    pub scan_error: bool,
    pub free_entries: usize,
    pub fail_commit: bool,
    pub commits: u32,
    pub fail_commit_at: u32,
    pub fail_erase_at: u32,
}
impl nvs::Native for Flash {
    fn storage_ready(&mut self) -> bool { !self.unavailable }
    fn open(&mut self, name: &CStr, readonly: bool, handle: &mut u32) -> core::result::Result<(), nvs::NativeError> {
        let name = name.to_str().unwrap();
        self.legacy_opens += u32::from(name.starts_with("nb"));
        if readonly && !self.spaces.contains_key(name) { return Err(nvs::NativeError::NOT_FOUND); }
        self.spaces.entry(name.into()).or_default();
        *handle = self.handles.len() as u32 + 1;
        self.handles.insert(*handle, name.into());
        Ok(())
    }
    fn close(&mut self, handle: u32) { assert!(self.handles.remove(&handle).is_some()); }
    fn get_blob(&mut self, handle: u32, key: &CStr, out: Option<&mut [u8]>, size: &mut usize)
        -> core::result::Result<(), nvs::NativeError> {
        self.reads += 1;
        let Some(bytes) = self.spaces[&self.handles[&handle]].get(key.to_str().unwrap())
            else { return Err(nvs::NativeError::NOT_FOUND) };
        if let Some(out) = out {
            assert!(*size >= bytes.len() && out.len() >= bytes.len());
            out[..bytes.len()].copy_from_slice(bytes);
        }
        *size = bytes.len();
        Ok(())
    }
    fn set_blob(&mut self, handle: u32, key: &CStr, bytes: &[u8]) -> core::result::Result<(), nvs::NativeError> {
        if self.require_reclaim_before_write {
            assert!(self.spaces.iter().all(|(name, keys)| !name.starts_with("nb") || keys.is_empty()),
                "reclaim must precede first settings write");
        }
        let name = &self.handles[&handle];
        assert_eq!(name, "newt_prefs", "migration must never write progress to NVS");
        let entries = 2 + bytes.len().div_ceil(32);
        if self.free_entries < entries { return nvs::NativeError::from_code(0x1105); }
        self.free_entries -= entries;
        if let Some(old) = self.spaces.get_mut(name).unwrap().insert(key.to_str().unwrap().into(), bytes.to_vec()) {
            self.free_entries += 2 + old.len().div_ceil(32);
        }
        Ok(())
    }
    fn commit(&mut self, _: u32) -> core::result::Result<(), nvs::NativeError> {
        self.commits += 1;
        if self.fail_commit || self.commits == self.fail_commit_at { nvs::NativeError::from_code(-1) }
        else { Ok(()) }
    }
    fn erase_key(&mut self, handle: u32, key: &CStr) -> core::result::Result<(), nvs::NativeError> {
        self.erased += 1;
        if self.erased == self.fail_erase_at { return nvs::NativeError::from_code(-1); }
        if let Some(old) = self.spaces.get_mut(&self.handles[&handle]).unwrap().remove(key.to_str().unwrap()) {
            self.free_entries += 2 + old.len().div_ceil(32);
        }
        Ok(())
    }
    fn readback_durable(&self) -> bool { true }
}

impl nvs::Inventory for Flash {
    fn visit_entries(&mut self, visitor: &mut dyn FnMut(&[u8], &[u8], bool) -> bool)
        -> core::result::Result<(), nvs::NativeError> {
        self.scans += 1;
        if self.scan_error { return nvs::NativeError::from_code(-1); }
        for (name, values) in &self.spaces {
            for key in values.keys() {
                if !visitor(name.as_bytes(), key.as_bytes(), !self.non_blob.as_ref().is_some_and(|(space, entry)| space == name && entry == key)) { return nvs::NativeError::from_code(-1); }
            }
        }
        Ok(())
    }
}
