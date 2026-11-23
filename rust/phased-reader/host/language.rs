// SPDX-License-Identifier: MIT
use crate::{entry::linebreak::NativeBreak, physical::{Bank, Key, Source, Store}};
use core::ffi::CStr;
use newt_core::{chapter_layout::types::LineBreak, preferences::ViewSettings, zip_container::Status};
use newt_phased_reader::{CacheRequest, OperationPhased, PhasedChapterCache, PhasedReader, Selection,
    imported_revision::session::{Build, Cache}, phase_storage::{BANK_BYTES, Storage}};
use newt_phased_sink::{Plan, RUNS, Run, TEXT, Target};
use std::path::Path;

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
        let mut source = Source::open(&root.join(format!("language-{name}.epub")))?;
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
                operation.reader.close();
                assert!(cache.stored(&digest, 0, &mut operation), "warm Japanese cache presence");
                assert_eq!(operation.reader.open_phased(&mut source, operation.storage, operation.lender, operation.backend), Status::Ok);
                assert!(cache.capture(&mut source, &digest, request, &mut operation).hit);

                let japanese_name = cache.store.name.clone();
                operation.reader.close();
                cache.store.name = previous_name;
                cache.store.file.clone_from(&english);
                assert!(cache.stored(&digest, 0, &mut operation), "old English file is a warm hint");
                assert_eq!(operation.reader.open_phased(&mut source, operation.storage, operation.lender, operation.backend), Status::Ok);
                assert!(!cache.capture(&mut source, &digest, request, &mut operation).hit,
                    "old English cache must miss after verified Japanese selection");
                assert_eq!(cache.build(&mut source, &digest, 0, &mut operation), Build::Ready);
                assert_eq!(cache.store.name, japanese_name);
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
