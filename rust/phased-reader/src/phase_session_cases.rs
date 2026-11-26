// SPDX-License-Identifier: MIT
//! Opening authority belongs to the visible reader, including segmented hits.
extern crate std;
use super::*;
use newt_core::cache_names::segment::SegmentAnchor;
use newt_core::page_cache::CacheStorage;
use std::boxed::Box;
use std::vec;

#[repr(align(16))]
struct Bank([u8; crate::phase_storage::BANK_BYTES]);

struct Ports {
    opening: Option<bool>,
}
impl CacheStorage for Ports {
    fn file_size(&mut self, _: &[u8]) -> i64 {
        panic!("scope refuses before file access")
    }
    fn read_at(&mut self, _: &[u8], _: u32, _: &mut [u8]) -> i32 {
        panic!("scope refuses before read")
    }
    fn remove(&mut self, _: &[u8]) -> bool {
        panic!("no cache mutation")
    }
    fn begin_write(&mut self, _: &[u8]) -> bool {
        panic!("no cache mutation")
    }
    fn write(&mut self, _: &[u8]) -> bool {
        panic!("no cache mutation")
    }
    fn end_write(&mut self) -> bool {
        panic!("no cache mutation")
    }
}
impl CachePresence for Ports {
    fn with_index<R>(&mut self, _: &[u8; 32], _: impl FnOnce(&mut dyn crate::index_build::Store) -> R) -> Option<R> { None }
    fn present(&mut self, _: &[u8], _: bool) -> bool {
        panic!("read_entry already selected the name")
    }
}
impl ReadScope for Ports {
    fn query<R>(
        &mut self,
        _: &[u8],
        opening: bool,
        _: &mut [u8],
        _: &mut dyn FnMut() -> bool,
        _: impl FnOnce(&mut dyn CacheStorage) -> R,
    ) -> Result<(R, bool), Status> {
        assert!(self.opening.replace(opening).is_none());
        Err(Status::NotFound)
    }
}
impl crate::hash_provider::CacheHash for Ports {
    fn hash(&mut self, _: &[u8], _: &mut Digest) -> bool {
        panic!("name is already resolved")
    }
}
impl ParserBackend for Ports {
    fn create(&mut self) -> bool {
        panic!("cache scope must not parse")
    }
    fn feed(&mut self, _: &[u8], _: bool, _: &mut dyn xml_session::Handler) -> bool {
        panic!("cache scope must not parse")
    }
    fn close(&mut self) {
        panic!("cache scope must not parse")
    }
}
impl LineBreak for Ports {
    fn breaks(&mut self, _: &[u8], _: Option<&core::ffi::CStr>, _: &mut [u8]) {
        panic!("cache scope must not lay out")
    }
}
impl BookSource for Ports {
    fn size(&self) -> u64 {
        0
    }
    fn read_at(&mut self, _: u64, _: &mut [u8]) -> i32 {
        panic!("refused cache scope never reads source")
    }
}
struct Loan(std::vec::Vec<u8>);
impl BuildLoan for Loan {
    fn with_parse<R>(&mut self, work: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> {
        Ok(work(&mut self.0))
    }
}

#[test]
fn segmented_and_whole_cache_reads_use_the_actual_opening_state() {
    for has_page in [false, true] {
        for located in [None, Some(SegmentAnchor::default())] {
            let mut selections = [Selection::default(), Selection::default()];
            let mut reader = PhasedReader::new(&mut selections, None, PageLayout::PORTRAIT);
            reader.has_page = has_page;
            let mut shared = Box::new(Bank([0; crate::phase_storage::BANK_BYTES]));
            let mut storage = Storage::new(&mut shared.0);
            let mut loan = Loan(vec![0; crate::phase_storage::BANK_BYTES + 15]);
            let (mut store, mut hash, mut parser, mut breaker, mut source) = (
                Ports { opening: None },
                Ports { opening: None },
                Ports { opening: None },
                Ports { opening: None },
                Ports { opening: None },
            );
            let mut operation = OperationPhased::new(
                &mut reader,
                &mut storage,
                &mut loan,
                &mut parser,
                &mut breaker,
            );
            let mut cache = PhasedChapterCache {
                store: &mut store,
                hash: &mut hash,
                settings: ViewSettings::default(),
            };
            let request = CacheRequest {
                spine: 0,
                target: 0,
                anchor: true,
                staged: false,
                build_on_miss: false,
                cache_only: true,
            };
            assert_eq!(
                cache.read_entry(
                    &mut source,
                    b"cache.fibp",
                    0,
                    request,
                    located,
                    &mut operation
                ),
                Status::NotFound
            );
            assert_eq!(store.opening, Some(!has_page));
        }
    }
}
