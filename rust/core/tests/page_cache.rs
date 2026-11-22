use newt_core::chapter_layout::types::{Image, Link, Params, Run};
use newt_core::page_cache::*;
use newt_core::zip_window::arena::Arena;
#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    reads: Vec<(u32, usize)>,
    trace: Vec<&'static str>,
    fail_at: usize,
    writes: usize,
    fail_end: bool,
}
impl CacheStorage for Store {
    fn remove(&mut self, _: &[u8]) -> bool {
        self.trace.push("remove");
        self.bytes.clear();
        true
    }
    fn file_size(&mut self, _: &[u8]) -> i64 {
        self.bytes.len() as i64
    }
    fn read_at(&mut self, _: &[u8], o: u32, b: &mut [u8]) -> i32 {
        self.reads.push((o, b.len()));
        let o = o as usize;
        if o >= self.bytes.len() {
            return 0;
        }
        let n = (self.bytes.len() - o).min(b.len()).min(1);
        b[..n].copy_from_slice(&self.bytes[o..o + n]);
        n as i32
    }
    fn begin_write(&mut self, _: &[u8]) -> bool {
        self.trace.push("begin");
        self.bytes.clear();
        true
    }
    fn write(&mut self, b: &[u8]) -> bool {
        self.trace.push("write");
        self.writes += 1;
        if self.writes == self.fail_at {
            return false;
        }
        self.bytes.extend_from_slice(b);
        true
    }
    fn end_write(&mut self) -> bool {
        self.trace.push("end");
        !self.fail_end
    }
}
fn empty_record(chars: u32) -> [u8; 10] {
    let mut b = [0; 10];
    b[..4].copy_from_slice(&chars.to_le_bytes());
    b
}
#[test]
fn literal_names_and_hash_inputs() {
    let mut b = [0; 21];
    assert!(page_cache_name(7, 0x89abcdef, &mut b));
    assert_eq!(&b[..20], b"s0007-89abcdef.fibp\0");
    assert!(page_cache_name(65535, 0, &mut b));
    assert_eq!(&b, b"s65535-00000000.fibp\0");
    let mut b = [1; 5];
    assert!(!page_cache_name(7, 0, &mut b));
    assert_eq!(&b, b"s000\0");
    assert!(!page_cache_name(7, 0, &mut []));
    let p = Params::default();
    let h = layout_generation_hash(&p, 123, false);
    assert_ne!(h, layout_generation_hash(&p, 123, true));
    let mut q = p;
    q.language = Some(core::ffi::CStr::from_bytes_until_nul(b"en\0discard").unwrap());
    assert_eq!(h, layout_generation_hash(&q, 123, false));
    q = p;
    q.focus = true;
    assert_ne!(h, layout_generation_hash(&q, 123, false));
}
#[test]
fn literal_empty_full_and_partial() {
    for partial in [false, true] {
        let mut s = Store::default();
        let mut w = PageCacheWriter::new(Profile::Small);
        let mut anchors = [Anchor::default(); 192];
        let m = WriterMemory {
            chunks: &mut [],
            anchors: &mut anchors,
        };
        assert!(w.begin(&mut s, b"x", 0x12345678, &m));
        w.set_total_chars(9);
        assert!(if partial {
            w.suspend(&mut s, b"x", &m, 3, 8)
        } else {
            w.finish(&mut s, b"x", &m)
        });
        let mut expected =
            b"FIBP\x03\0\0\0\x78\x56\x34\x12\x0c\0\0\0\0\0\0\0\x0c\0\0\0\0\0\0\0\x09\0\0\0"
                .to_vec();
        if partial {
            expected.extend_from_slice(b"\x03\0\0\0\x08\0\0\0FIBx");
        } else {
            expected.extend_from_slice(b"FIBX");
        }
        assert_eq!(s.bytes, expected);
        let mut r = PageCacheReader::default();
        let mut n = [0; 2];
        let mut n = Arena::new(&mut n);
        let mut rm = ReaderMemory {
            pages: &mut [],
            anchors: &mut [],
        };
        assert_eq!(
            r.open(&mut s, b"x", 0x12345678, &mut n, &mut rm),
            Status::Ok
        );
        assert_eq!(r.is_partial(), partial);
        assert_eq!(r.total_chars(), 9);
    }
}
#[test]
fn failure_publication_order() {
    for fail in [1, 2, 3] {
        let mut s = Store {
            fail_at: fail,
            ..Store::default()
        };
        let mut w = PageCacheWriter::new(Profile::Small);
        let mut anchors = [Anchor::default(); 192];
        let m = WriterMemory {
            chunks: &mut [],
            anchors: &mut anchors,
        };
        let begun = w.begin(&mut s, b"x", 0, &m);
        assert_eq!(begun, fail != 1);
        let result = w.finish(&mut s, b"x", &m);
        if fail == 1 {
            assert!(!result);
            assert_eq!(s.trace, vec!["begin", "write", "end", "remove"]);
        }
        if fail == 2 {
            assert!(!result);
            assert_eq!(s.trace, vec!["begin", "write", "write", "remove"]);
        }
        if fail == 3 {
            assert!(result);
            assert_eq!(s.trace, vec!["begin", "write", "write", "end"]);
        }
    }
}
#[test]
fn chunk_boundary_duplicate_anchor_and_unsorted_lookup() {
    let mut s = Store::default();
    let mut w = PageCacheWriter::new(Profile::Small);
    let mut chunks = [IndexChunk::default()];
    let mut anchors = [Anchor::default(); 192];
    let mut m = WriterMemory {
        chunks: &mut chunks,
        anchors: &mut anchors,
    };
    assert!(w.begin(&mut s, b"x", 0, &m));
    for i in 0..200 {
        w.on_anchor(&mut m, 7, i);
    }
    assert_eq!(w.char_for_anchor(&m, 7), Some(0));
    assert_eq!(w.char_for_anchor(&m, 8), None);
    let (mut raw, mut runs, mut images, mut links) = (
        [0; 64],
        [Run::default(); 0],
        [Image::default(); 0],
        [Link::default(); 0],
    );
    let mut a = Arena::new(&mut raw);
    let mut pm = PageMemory {
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
    };
    for i in 0..129 {
        a.reset();
        let b = empty_record(i);
        let p = decode_record(&b, 0, &mut a, &mut pm).unwrap();
        assert_eq!(w.on_page(&mut s, &mut m, &p), i < 128);
    }
    assert!(w.failed());
    assert_eq!(w.page_count(), 128);
    assert_eq!(w.page_for_char(&m, 1000), 127);
    assert_eq!(w.char_start(&m, 128), 0);
}
#[test]
fn corrupt_vs_allocation_precedence_and_trailing_bytes() {
    let mut raw = [0; 64];
    let mut a = Arena::new(&mut raw);
    let mut pm = PageMemory {
        runs: &mut [],
        images: &mut [],
        links: &mut [],
    };
    let mut bad = [0; 10];
    bad[4] = 1;
    assert!(matches!(
        decode_record(&bad, 0, &mut a, &mut pm),
        Err(Status::OutOfMemory)
    ));
    a.reset();
    let mut runs = [Run::default(); 1];
    let mut pm = PageMemory {
        runs: &mut runs,
        images: &mut [],
        links: &mut [],
    };
    assert!(matches!(
        decode_record(&bad, 0, &mut a, &mut pm),
        Err(Status::Stale)
    ));
    a.reset();
    let mut trailing = empty_record(42).to_vec();
    trailing.extend_from_slice(b"ignored");
    let p = decode_record(&trailing, 4, &mut a, &mut pm).unwrap();
    assert_eq!((p.index, p.char_start), (4, 42));
    let mut tiny = [];
    let mut tiny = Arena::new(&mut tiny);
    assert!(matches!(
        decode_record(&[], 0, &mut tiny, &mut pm),
        Err(Status::Stale)
    ));
}

#[test]
fn admission_shortages_precede_io_and_page_failure_counts_are_published() {
    let mut s = Store::default();
    let mut w = PageCacheWriter::new(Profile::Small);
    let m = WriterMemory {
        chunks: &mut [],
        anchors: &mut [],
    };
    assert!(!w.begin(&mut s, b"x", 0, &m));
    assert!(s.trace.is_empty());
    let mut r = PageCacheReader::default();
    let mut raw = [];
    let mut names = Arena::new(&mut raw);
    let mut rm = ReaderMemory {
        pages: &mut [],
        anchors: &mut [],
    };
    assert_eq!(
        r.open(&mut s, b"x", 0, &mut names, &mut rm),
        Status::OutOfMemory
    );
    let mut anchors = [Anchor::default(); 192];
    let mut chunks = [IndexChunk::default()];
    let mut m = WriterMemory {
        chunks: &mut chunks,
        anchors: &mut anchors,
    };
    s.fail_at = 2;
    assert!(w.begin(&mut s, b"x", 0, &m));
    let p = newt_core::chapter_layout::types::Page {
        bytes: &[],
        runs: &[],
        images: &[],
        links: &[],
        index: 99,
        char_start: 7,
    };
    assert!(!w.on_page(&mut s, &mut m, &p));
    assert_eq!(w.page_count(), 1);
    assert_eq!(w.char_start(&m, 0), 7);
    assert!(!w.finish(&mut s, b"x", &m));
    assert!(s.trace.ends_with(&["end", "remove"]));
}

#[test]
fn link_allocation_follows_record_validation_and_image_decode() {
    // Header + one valid image + one link whose rectangle is truncated.
    let data = b"\0\0\0\0\0\0\x01\0\x01\0\0\0\0\0\x01\0\x01\0\x01\0x";
    let mut bytes = [0; 128];
    let mut arena = Arena::new(&mut bytes);
    let mut images = [Image::default(); 1];
    let mut pm = PageMemory {
        runs: &mut [],
        images: &mut images,
        links: &mut [],
    };
    assert!(matches!(
        decode_record(data, 0, &mut arena, &mut pm),
        Err(Status::OutOfMemory)
    ));
    assert_eq!(arena.used(), data.len() + 2); // image string was copied before missing link slots
    arena.reset();
    let mut links = [Link::default(); 1];
    let mut pm = PageMemory {
        runs: &mut [],
        images: &mut images,
        links: &mut links,
    };
    assert!(matches!(
        decode_record(data, 0, &mut arena, &mut pm),
        Err(Status::Stale)
    ));
}

#[test]
fn counted_reader_short_backing_preserves_error_and_read_order() {
    struct Allocation<'a> {
        pages: Option<&'a mut [Entry]>,
        anchors: Option<&'a mut [Anchor]>,
        calls: Vec<&'static str>,
    }
    impl<'a> IndexAllocation<'a> for Allocation<'a> {
        fn pages(&mut self, count: usize) -> Result<&'a mut [Entry], Status> {
            assert_eq!(count, 1);
            self.calls.push("pages");
            Ok(self.pages.take().unwrap())
        }
        fn anchors(&mut self, count: usize) -> Result<&'a mut [Anchor], Status> {
            assert_eq!(count, 1);
            self.calls.push("anchors");
            Ok(self.anchors.take().unwrap())
        }
    }
    for partial in [false, true] {
        // Independently authored one-page/one-anchor v3 record: offsets22/30.
        let mut bytes = b"FIBP\x03\0\0\0\x78\x56\x34\x12".to_vec();
        bytes.extend_from_slice(&[0; 10]);
        for value in [12u32, 0, 7, 0, 22, 1, 30, 1, 9] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        if partial {
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(&8u32.to_le_bytes());
        }
        bytes.extend_from_slice(if partial { b"FIBx" } else { b"FIBX" });
        assert_eq!(bytes.len(), if partial { 70 } else { 62 });
        for (page_len, anchor_len, expected) in [
            (0, 1, Status::OutOfMemory),
            (1, 0, Status::OutOfMemory),
            (1, 1, Status::Ok),
            (2, 2, Status::Ok),
        ] {
            let mut fixed = Store {
                bytes: bytes.clone(),
                ..Store::default()
            };
            let mut allocated = Store {
                bytes: bytes.clone(),
                ..Store::default()
            };
            let (mut pages, mut anchors) = ([Entry::default(); 2], [Anchor::default(); 2]);
            let mut memory = ReaderMemory {
                pages: &mut pages[..page_len],
                anchors: &mut anchors[..anchor_len],
            };
            let mut name = [0; 2];
            let mut reader = PageCacheReader::default();
            assert_eq!(
                reader.open(
                    &mut fixed,
                    b"x",
                    0x12345678,
                    &mut Arena::new(&mut name),
                    &mut memory
                ),
                expected
            );
            let (mut pages, mut anchors) = ([Entry::default(); 2], [Anchor::default(); 2]);
            let mut allocation = Allocation {
                pages: Some(&mut pages[..page_len]),
                anchors: Some(&mut anchors[..anchor_len]),
                calls: Vec::new(),
            };
            let mut name = [0; 2];
            let mut reader = PageCacheReader::default();
            let result = reader.open_allocated(
                &mut allocated,
                b"x",
                0x12345678,
                &mut Arena::new(&mut name),
                &mut allocation,
            );
            match result {
                Ok(memory) => {
                    assert_eq!(expected, Status::Ok);
                    assert_eq!(reader.page_count(), 1);
                    assert_eq!(reader.char_for_anchor(&memory, 7), Some(0));
                }
                Err(status) => {
                    assert_eq!(status, expected);
                    assert_eq!(reader.page_count(), 0);
                }
            }
            assert_eq!(allocated.reads, fixed.reads);
            assert_eq!(
                allocation.calls,
                if page_len == 0 {
                    vec!["pages"]
                } else {
                    vec!["pages", "anchors"]
                }
            );
            if page_len == 0 {
                assert!(allocated.reads.iter().all(|(at, _)| !(22..38).contains(at)));
            } else if anchor_len == 0 {
                assert!(allocated.reads.iter().any(|(at, _)| *at == 22));
                assert!(allocated.reads.iter().all(|(at, _)| !(30..38).contains(at)));
            }
        }
    }
}
