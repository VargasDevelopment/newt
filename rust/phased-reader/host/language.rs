// SPDX-License-Identifier: MIT
use crate::entry::linebreak::NativeBreak;
use core::ffi::CStr;
use newt_core::{chapter_layout::types::LineBreak, page_cache::CacheStorage,
    preferences::ViewSettings, zip_container::Status, zip_window::source::BookSource};
use newt_phased_reader::{CachePresence, CacheRequest, OperationPhased, PhasedChapterCache,
    PhasedReader, Selection, hash_provider::CacheHash, imported_revision::session::{Build, Cache},
    phase_storage::{BANK_BYTES, BuildLoan, ReadScope, Storage}};
use newt_phased_sink::{Plan, RUNS, Run, TEXT, Target};
use std::{io::Read, path::Path};

struct Source(std::io::Cursor<Vec<u8>>);
impl BookSource for Source {
    fn size(&self) -> u64 { self.0.get_ref().len() as u64 }
    fn read_at(&mut self, at: u64, bytes: &mut [u8]) -> i32 {
        self.0.set_position(at);
        self.0.read(bytes).map_or(-1, |n| n as i32)
    }
}

#[repr(align(16))]
struct Bank([u8; BANK_BYTES]);
impl BuildLoan for Bank {
    fn with_parse<R>(&mut self, operation: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> {
        let result = operation(&mut self.0);
        self.0.fill(0);
        Ok(result)
    }
}

#[derive(Default)]
struct Store {
    name: Vec<u8>,
    file: Vec<u8>,
    stale: Vec<u8>,
}
impl CacheStorage for Store {
    fn file_size(&mut self, name: &[u8]) -> i64 {
        if self.present(name, false) { self.file.len() as i64 } else { -1 }
    }
    fn read_at(&mut self, name: &[u8], at: u32, bytes: &mut [u8]) -> i32 {
        if name != self.name { return -1; }
        self.file.get(at as usize..).map_or(-1, |mut file| file.read(bytes).map_or(-1, |n| n as i32))
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.name = name.to_vec();
        self.file.clear();
        true
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.file.extend_from_slice(bytes);
        true
    }
    fn end_write(&mut self) -> bool { true }
    fn remove(&mut self, _: &[u8]) -> bool { false }
}
impl ReadScope for Store {
    fn query<R>(&mut self, name: &[u8], _: bool, _: &mut [u8], live: &mut dyn FnMut() -> bool,
        read: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        if !live() || !self.present(name, false) { return Err(Status::NotFound); }
        Ok((read(self), live()))
    }
}
impl CachePresence for Store {
    fn present(&mut self, name: &[u8], _: bool) -> bool { (self.name == name && !self.file.is_empty()) || (!self.stale.is_empty() && self.stale == name) }
}

struct Key;
impl CacheHash for Key {
    fn hash(&mut self, bytes: &[u8], key: &mut [u8; 32]) -> bool {
        let Some(layout) = bytes.get(84..99) else { return false; };
        key[..layout.len()].copy_from_slice(layout);
        true
    }
}

struct Breaks {
    expected: &'static CStr,
    seen: usize,
}
impl LineBreak for Breaks {
    fn breaks(&mut self, text: &[u8], tag: Option<&CStr>, out: &mut [u8]) {
        assert_eq!(tag, Some(self.expected), "OPF language did not reach line breaking");
        NativeBreak.breaks(text, tag, out);
        let mut english = vec![0; text.len()];
        NativeBreak.breaks(text, Some(c"en"), &mut english);
        assert_eq!(out == english, self.expected == c"en", "tailored break positions");
        self.seen += 1;
    }
}

pub fn run(root: &Path) -> std::io::Result<()> {
    let mut shared = Box::new(Bank([0; BANK_BYTES]));
    let mut native = Box::new(Bank([0; BANK_BYTES]));
    let mut storage = Storage::new(&mut shared.0);
    let mut parser = newt_target_expat::Expat::default();
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [Run::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), true);
    let mut store = Store::default();
    let mut english = Vec::new();
    // Hold the digest fixed to isolate language as the only cache-key input.
    let digest = [0; 32];
    let request = CacheRequest { spine: 0, target: 0, anchor: false, staged: false,
        build_on_miss: false, cache_only: true };
    for (name, tag, previous_hit) in [
        ("en", c"en", false), ("ja", c"ja", false), ("zh", c"zh", false),
        ("ko", c"ko", false), ("missing", c"en", false), ("invalid", c"en", true),
        ("empty", c"en", true), ("unknown", c"en", true), ("overlong", c"en", true),
    ] {
        let bytes = std::fs::read(root.join(format!("language-{name}.epub")))?;
        let mut source = Source(std::io::Cursor::new(bytes));
        let previous_name = store.name.clone();
        let mut breaks = Breaks { expected: tag, seen: 0 };
        assert_eq!(reader.open_phased(&mut source, &mut storage, &mut *native, &mut parser), Status::Ok);
        {
            let mut operation = OperationPhased::new(&mut reader, &mut storage, &mut *native, &mut parser, &mut breaks);
            let mut cache = PhasedChapterCache { store: &mut store, hash: &mut Key, settings: ViewSettings::default() };
            let hit = cache.capture(&mut source, &digest, request, &mut operation).hit;
            assert_eq!(hit, previous_hit, "{name}: cache from previous language");
            if hit {
                // Reopen so the pending cache hit does not preclude a build.
                assert_eq!(operation.reader.open_phased(&mut source, operation.storage, operation.lender, operation.backend), Status::Ok);
            }
            assert_eq!(cache.build(&mut source, &digest, 0, &mut operation), Build::Ready);
            if name == "ja" {
                for stale in [&[][..], &previous_name] {
                    operation.reader.close();
                    cache.store.stale = stale.to_vec();
                    assert!(cache.stored(&digest, 0, &mut operation), "warm Japanese cache presence");
                    assert_eq!(operation.reader.open_phased(&mut source, operation.storage, operation.lender, operation.backend), Status::Ok);
                    assert!(cache.capture(&mut source, &digest, request, &mut operation).hit,
                        "verified selection opens the Japanese file even with a stale English hint");
                }
                cache.store.stale.clear();
                assert_eq!(operation.reader.open_phased(&mut source, operation.storage, operation.lender, operation.backend), Status::Ok);
            }
        }
        assert!(breaks.seen > 0, "{name}: cache build must break text");
        if name == "en" { english.clone_from(&store.file); }
        if tag == c"en" { assert_eq!(store.file, english, "{name}: English-compatible pages"); }
        breaks.seen = 0;
        assert_eq!(reader.capture_phased(&mut source, 0, 0, Target::Page, &mut storage,
            &mut *native, &mut parser, &mut breaks), Status::Ok);
        assert!(breaks.seen > 0, "{name}: ordinary layout must break text");
        assert_eq!(reader.publish(&mut source, &mut [0; newt_phased_sink::FRAME_BYTES]), Status::Ok);
    }
    println!("PASS declared language reaches native line breaking and cache identity");
    Ok(())
}
