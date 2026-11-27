use newt_core::chapter_layout::types::{Bytes, Image, Link, Page, Run};
use newt_core::page_cache::*;
use newt_core::page_cache_access::VerifiedAccess;
use newt_core::page_envelope::{self, BLOCK_BYTES, Query, RawRead, VerifiedStore};
use newt_core::resume_point::*;
use newt_core::xml_input::EntityFilter;
use newt_core::zip_window::arena::Arena;

fn header(mode: u8, lengths: [u32; 7]) -> [u8; HEADER_BYTES] {
    let mut h = [0; HEADER_BYTES];
    h[..4].copy_from_slice(&MAGIC);
    h[4..6].copy_from_slice(&VERSION.to_le_bytes());
    h[6] = mode;
    for (i, length) in lengths.into_iter().enumerate() {
        h[8 + 4 * i..12 + 4 * i].copy_from_slice(&length.to_le_bytes());
    }
    h
}

fn total(lengths: [u32; 7]) -> u32 {
    HEADER_BYTES as u32 + lengths.iter().sum::<u32>() + TRAILER_BYTES
}

#[test]
fn sections_are_contiguous_in_consumption_order() {
    let lengths = [40, 12, 30, 9000, 0, 7, 300];
    let layout = Layout::parse(&header(1, lengths), total(lengths)).unwrap();
    assert_eq!(layout.mode, Mode::Reinflate);
    let mut at = HEADER_BYTES as u32;
    for (section, length) in SECTIONS.into_iter().zip(lengths) {
        assert_eq!(layout.range(section), (at, length), "{section:?}");
        at += length;
    }
    assert_eq!(layout.trailer(), at);
    assert_eq!(at + TRAILER_BYTES, total(lengths));
    // Restore reads the parser input before the engine, then the feed bytes.
    assert_eq!(&SECTIONS[1..4], &[Section::Source, Section::Stream, Section::Engine]);
    assert_eq!(&SECTIONS[4..], &[Section::Prolog, Section::Names, Section::Replay]);
}

#[test]
fn every_header_field_and_bound_is_checked() {
    let lengths = [1, 1, 1, 1, 0, 1, 0];
    let point = total(lengths);
    assert_eq!(Layout::parse(&header(0, lengths), point).unwrap().mode, Mode::Seek);
    assert_eq!(Layout::parse(&header(0, lengths), point + 1), None);
    assert_eq!(Layout::parse(&header(2, lengths), point), None);
    for at in [0, 4, 7] {
        let mut bad = header(0, lengths);
        bad[at] ^= 1;
        assert_eq!(Layout::parse(&bad, point), None, "byte {at}");
    }
    for (i, section) in SECTIONS.into_iter().enumerate() {
        let mut over = lengths;
        over[i] = section.max_bytes() + 1;
        assert_eq!(Layout::parse(&header(0, over), total(over)), None, "{section:?} over");
        let mut full = lengths;
        full[i] = section.max_bytes();
        assert!(Layout::parse(&header(0, full), total(full)).is_some(), "{section:?} at cap");
        if !matches!(section, Section::Prolog | Section::Replay) {
            let mut empty = lengths;
            empty[i] = 0;
            assert_eq!(Layout::parse(&header(0, empty), total(empty)), None, "{section:?} empty");
        }
    }
}

#[test]
fn the_stream_section_holds_the_whole_entity_filter() {
    // Host layout, usize = 8: an upper bound on any packed encoding of the
    // filter, plus the two u64 stream counters.
    let state = size_of::<EntityFilter>() + 2 * size_of::<u64>();
    assert!(state >= 512 + 32 + 13, "the filter keeps its raw, pending and entity buffers");
    assert!(state <= Section::Stream.max_bytes() as usize, "{state} B");
}

#[test]
fn layout_from_lengths_and_write_header_roundtrip() {
    let lengths = [BINDING_BYTES as u32, SOURCE_BYTES as u32, 32, 4096, 0, 12, 51];
    for mode in [Mode::Seek, Mode::Reinflate] {
        let (layout, total_bytes) = Layout::from_lengths(mode, lengths).unwrap();
        assert_eq!(total_bytes, total(lengths));
        let mut raw = [0u8; HEADER_BYTES];
        layout.write_header(&mut raw);
        assert_eq!(Layout::parse(&raw, total_bytes), Some(layout));
    }
    let mut bad = lengths;
    bad[Section::Engine as usize] = Section::Engine.max_bytes() + 1;
    assert_eq!(Layout::from_lengths(Mode::Seek, bad), None);
    let mut empty_req = lengths;
    empty_req[Section::Names as usize] = 0;
    assert_eq!(Layout::from_lengths(Mode::Seek, empty_req), None);
}

#[test]
fn binding_source_and_crc_out_encode_exact_lengths_and_crc() {
    let binding = Binding {
        book_digest: [0x3c; 32],
        spine: 3,
        generation: 0xdead_beef,
        segment_index: 2,
        page_count: 512,
        total_chars: 98_765,
        style_hash: 0x1122_3344,
        style_count: 14,
        style_issues: 2,
        style_state: 3,
        profile: 0,
        member_method: 8,
        member_offset: 1234,
        member_compressed: 5678,
        member_uncompressed: 9999,
        source_bytes: 0x0102_0304_0506_0708,
        pages_at: 254,
        cut: 40_000,
        fed: 40_051,
    };
    let mut m = Measure::default();
    assert!(binding.encode(&mut m));
    assert_eq!(m.bytes as usize, BINDING_BYTES);
    let mut sm = Measure::default();
    assert!(encode_source(12_345, &mut sm));
    assert_eq!(sm.bytes as usize, SOURCE_BYTES);

    let mut out = Vec::new();
    let expected = (BINDING_BYTES + SOURCE_BYTES) as u32;
    let crc = {
        let mut write = |bytes: &[u8]| {
            out.extend_from_slice(bytes);
            true
        };
        let mut crc_out = CrcOut::new(expected, &mut write);
        assert!(binding.encode(&mut crc_out));
        assert!(encode_source(12_345, &mut crc_out));
        assert!(!crc_out.put(&[0]), "overshoot past limit must fail closed");
        assert!(crc_out.failed);
        crc_out.finish_crc()
    };
    assert_eq!(out.len(), expected as usize);
    assert_eq!(&out[..4], b"BND1");
    assert_eq!(&out[4..36], &[0x3c; 32]);
    assert_eq!(&out[BINDING_BYTES..BINDING_BYTES + 4], b"SRC1");
    assert_eq!(
        u32::from_le_bytes(out[BINDING_BYTES + 4..BINDING_BYTES + 8].try_into().unwrap()),
        12_345
    );
    assert_eq!(crc, page_envelope::crc(u32::MAX, &out) ^ u32::MAX);

    let mut fail_write = |_bytes: &[u8]| false;
    let mut failed = CrcOut::new(16, &mut fail_write);
    assert!(!failed.put(b"abcd"));
    assert!(failed.failed);
    assert_eq!(failed.bytes, 0);
}

#[test]
fn utf8_prolog_checks_leading_bytes_even_when_prolog_is_empty() {
    // Empty prolog: root starts at byte 0. Still inspects original leading bytes!
    assert_eq!(validate_utf8_prolog(b"<htm", b""), Ok(()));
    assert_eq!(validate_utf8_prolog(b"", b""), Err(Decline::Encoding));
    assert_eq!(validate_utf8_prolog(b"<\0h\0", b""), Err(Decline::Encoding));
    assert_eq!(validate_utf8_prolog(b"\0<\0h", b""), Err(Decline::Encoding));
    assert_eq!(validate_utf8_prolog(&[0xff, 0xfe, b'<', 0], b""), Err(Decline::Encoding));
    assert_eq!(validate_utf8_prolog(&[0xfe, 0xff, 0, b'<'], b""), Err(Decline::Encoding));
    assert_eq!(validate_utf8_prolog(&[0x4c, 0x6f, 0xa7, 0x94], b""), Err(Decline::Encoding));
    assert_eq!(validate_utf8_prolog(b" <ht", b""), Err(Decline::Encoding));
    // XMLDecl encoding checks: accept omitted or case-insensitive UTF-8; decline all others.
    assert_eq!(
        validate_utf8_prolog(b"<?xm", b"<?xml version='1.0' encoding = 'ISO-8859-1'?>"),
        Err(Decline::Encoding)
    );
    assert_eq!(
        validate_utf8_prolog(b"<?xm", b"<?xml version=\"1.0\" encoding=\"US-ASCII\"?>"),
        Err(Decline::Encoding)
    );
    assert_eq!(
        validate_utf8_prolog(
            &[0xef, 0xbb, 0xbf, b'<'],
            b"\xef\xbb\xbf<?xml version='1.0' encoding='ISO-8859-1'?>"
        ),
        Err(Decline::Encoding)
    );
    assert_eq!(
        validate_utf8_prolog(b"<?xm", b"<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"no\" ?>"),
        Ok(())
    );
    assert_eq!(
        validate_utf8_prolog(b"<?xm", b"<?xml version=\"1.0\" standalone='yes' ?>"),
        Ok(())
    );
    assert_eq!(
        validate_utf8_prolog(b"<?xm", b"<?xml version='1.0' encoding='UTF-8' extra='1'?>"),
        Err(Decline::Encoding)
    );
    // UTF-8 BOM and xml-stylesheet PI are allowed; internal DTD subset and overlong prolog decline.
    assert_eq!(
        validate_utf8_prolog(&[0xef, 0xbb, 0xbf, b'<'], b"\xef\xbb\xbf<?xml version='1.0'?>"),
        Ok(())
    );
    assert_eq!(
        validate_utf8_prolog(b"<?xm", b"<?xml-stylesheet href='a.css'?>"),
        Ok(())
    );
    assert_eq!(
        validate_utf8_prolog(b"<!DO", b"<!DOCTYPE html [ <!ENTITY a 'b'> ]>"),
        Err(Decline::Prolog)
    );
    assert_eq!(
        validate_utf8_prolog(b"<!--", &[b' '; PROLOG_MAX + 1]),
        Err(Decline::Prolog)
    );
    assert_eq!(
        validate_utf8_prolog(b"<!--", b"<!--\0-->"),
        Err(Decline::Encoding)
    );
}

#[test]
fn page_cache_writer_v4_point_roundtrip_read_back_and_fault_abort() {
    #[derive(Default)]
    struct MemStore {
        temp: Vec<u8>,
        published: Option<Vec<u8>>,
        fail_after: Option<usize>,
        aborted: bool,
        ended: bool,
    }
    impl MemStore {
        fn active(&self) -> &[u8] {
            self.published.as_deref().unwrap_or(&self.temp)
        }
    }
    impl CacheStorage for MemStore {
        fn remove(&mut self, _: &[u8]) -> bool {
            // Mirrors native ChapterCacheAccess::remove: closes/aborts only an
            // unpublished temp and cannot undo an already-promoted publication.
            self.aborted = true;
            self.temp.clear();
            true
        }
        fn file_size(&mut self, _: &[u8]) -> i64 {
            self.active().len() as i64
        }
        fn read_at(&mut self, _: &[u8], off: u32, out: &mut [u8]) -> i32 {
            let src = self.active();
            let at = off as usize;
            let n = out.len().min(src.len().saturating_sub(at));
            out[..n].copy_from_slice(&src[at..at + n]);
            n as i32
        }
        fn begin_write(&mut self, _: &[u8]) -> bool {
            self.temp.clear();
            self.published = None;
            self.aborted = false;
            self.ended = false;
            true
        }
        fn write(&mut self, b: &[u8]) -> bool {
            if let Some(cap) = self.fail_after {
                if self.temp.len() + b.len() > cap {
                    return false;
                }
            }
            self.temp.extend_from_slice(b);
            true
        }
        fn end_write(&mut self) -> bool {
            self.ended = true;
            self.published = Some(self.temp.clone());
            true
        }
        fn read_back_at(&mut self, off: u32, out: &mut [u8]) -> i32 {
            let at = off as usize;
            let n = out.len().min(self.temp.len().saturating_sub(at));
            out[..n].copy_from_slice(&self.temp[at..at + n]);
            n as i32
        }
    }

    const NAME: &[u8; 69] =
        b"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff.fibp";
    let make_page = |idx: u32, text: &'static [u8]| {
        (
            [Run { text: Bytes { start: 0, len: text.len() as u16 }, x: 4, baseline: 12, size: 16, flags: 0 }],
            text,
            idx,
        )
    };
    let (r0, t0, _) = make_page(0, b"first-page-body");
    let (r1, t1, _) = make_page(1, b"second-page-body");
    let p0 = Page { bytes: t0, runs: &r0, images: &[], links: &[], index: 0, char_start: 0 };
    let p1 = Page { bytes: t1, runs: &r1, images: &[], links: &[], index: 1, char_start: 100 };

    // 1. v4 with point between page 0 and page 1: read_page while open caps page 0 at point,
    //    and sealed payload verifies through VerifiedStore + PageCacheReader.
    let mut store = MemStore::default();
    let mut chunks = [IndexChunk::default(); 1];
    let mut anchors = [Anchor::default(); 192];
    let mut wm = WriterMemory { chunks: &mut chunks, anchors: &mut anchors };
    let mut writer = PageCacheWriter::new(Profile::Small);
    assert!(writer.begin_v4(&mut store, NAME, 0x1234_5678, &wm));
    assert!(writer.is_v4());
    assert!(writer.on_page(&mut store, &mut wm, &p0));
    let lengths = [4, 4, 4, 4, 0, 1, 0];
    let (layout, point_len) = Layout::from_lengths(Mode::Seek, lengths).unwrap();
    let mut raw_point = Vec::new();
    let mut write_cb = |bytes: &[u8]| {
        raw_point.extend_from_slice(bytes);
        store.write(bytes)
    };
    let mut emit = |crc_out: &mut CrcOut<'_>| {
        let mut h = [0u8; HEADER_BYTES];
        layout.write_header(&mut h);
        crc_out.put(&h)
            && crc_out.put(b"BND1SRC1XST1ENG1\x00")
            && crc_out.put(&crc_out.finish_crc().to_le_bytes())
    };
    assert!(writer.write_point(point_len, &mut write_cb, &mut emit));
    assert!(writer.has_point());
    assert_eq!(raw_point.len(), point_len as usize);
    // Open-writer read_page caps page 0 at point.offset and decodes with exact=true
    // for v4 (so swallowing trailing point bytes or stray record bytes returns Stale).
    let mut scratch_buf = [0u8; 512];
    let mut scratch = Arena::new(&mut scratch_buf);
    let mut runs_out = [Run::default(); 4];
    let mut imgs_out = [Image::default(); 1];
    let mut links_out = [Link::default(); 1];
    let mut pm = PageMemory { runs: &mut runs_out, images: &mut imgs_out, links: &mut links_out };
    let read0 = writer.read_page(&mut store, &wm, 0, &mut scratch, &mut pm).unwrap();
    assert_eq!(read0.runs.len(), 1);
    assert_eq!(read0.runs[0].text.get(read0.bytes), t0);
    // Corrupt page 0's run length down by 1 byte so 1 trailing byte sits before point.offset;
    // v4 exact decoding must reject the open-writer readback with Status::Stale.
    let saved_len_byte = store.temp[12 + 10 + 8];
    store.temp[12 + 10 + 8] = saved_len_byte - 1;
    let mut bad_scratch_buf = [0u8; 512];
    let mut bad_scratch = Arena::new(&mut bad_scratch_buf);
    assert_eq!(
        writer.read_page(&mut store, &wm, 0, &mut bad_scratch, &mut pm).err(),
        Some(Status::Stale),
        "v4 open-writer read_page must enforce exact extent decoding"
    );
    store.temp[12 + 10 + 8] = saved_len_byte;

    assert!(writer.on_page(&mut store, &mut wm, &p1));
    writer.set_total_chars(200);
    assert!(writer.finish(&mut store, NAME, &wm));

    // Wrap sealed v4 payload in real verified envelope and inspect via PageCacheReader.
    let payload = store.published.clone().expect("finished segment must publish");
    let route = Query::new(Some(NAME)).route;
    let mut dir = Vec::new();
    for (block, bytes) in payload.chunks(BLOCK_BYTES as usize).enumerate() {
        dir.extend_from_slice(&page_envelope::block_crc(&route, block as u32, bytes).to_le_bytes());
    }
    let dir_crc = page_envelope::crc(u32::MAX, &dir) ^ u32::MAX;
    let mut manifest = [0u8; 64];
    page_envelope::manifest(&mut manifest, payload.len() as u32, &route, dir_crc, 0);
    let env_bytes = [payload.as_slice(), &dir, &manifest].concat();
    struct SliceRaw(Vec<u8>);
    impl RawRead for SliceRaw {
        fn read(&mut self, _: &Query<'_>, off: u32, out: &mut [u8]) -> bool {
            let at = off as usize;
            self.0.get(at..at + out.len()).map(|s| out.copy_from_slice(s)).is_some()
        }
    }
    let mut raw = SliceRaw(env_bytes);
    let (mut dir_buf, mut win_buf) = ([0u8; 4160], [0u8; 4096]);
    let mut vs = VerifiedStore::new(Some(NAME), &mut dir_buf, &mut win_buf);
    vs.query.extent = raw.0.len() as u32;
    vs.query.bound = true;
    assert!(vs.attach(&mut raw));
    let mut access = VerifiedAccess { store: &mut vs, source: &mut raw };
    let mut name_buf = [0u8; 128];
    let mut names = Arena::new(&mut name_buf);
    let mut pages_idx = [Entry::default(); 4];
    let mut anchors_idx = [Anchor::default(); 4];
    let mut rm = ReaderMemory { pages: &mut pages_idx, anchors: &mut anchors_idx };
    let mut reader = PageCacheReader::default();
    assert_eq!(reader.open(&mut access, NAME, 0x1234_5678, &mut names, &mut rm), Status::Ok);
    assert_eq!(reader.page_count(), 2);
    let pr = reader.point().expect("v4 segment must expose verified point range");
    assert_eq!(pr.length, point_len);
    let mut got_point = vec![0u8; pr.length as usize];
    assert_eq!(access.read_at(NAME, pr.offset, &mut got_point), pr.length as i32);
    assert_eq!(got_point, raw_point);
    for (idx, expected_text) in [(0, t0), (1, t1)] {
        let mut sb = [0u8; 512];
        let mut sa = Arena::new(&mut sb);
        let mut r = [Run::default(); 4];
        let mut im = [Image::default(); 1];
        let mut lk = [Link::default(); 1];
        let mut pm = PageMemory { runs: &mut r, images: &mut im, links: &mut lk };
        let got = reader.read_page(&mut access, &rm, &names, idx, &mut sa, &mut pm).unwrap();
        assert_eq!(got.runs[0].text.get(got.bytes), expected_text);
    }

    // 2. v4 with no point (point_length == 0): still readable and reports point() == None.
    let mut store2 = MemStore::default();
    assert!(writer.begin_v4(&mut store2, NAME, 0x1234_5678, &wm));
    assert!(writer.on_page(&mut store2, &mut wm, &p0));
    assert!(writer.finish(&mut store2, NAME, &wm));
    let mut reader2 = PageCacheReader::default();
    let mut nb2 = [0u8; 128];
    let mut na2 = Arena::new(&mut nb2);
    assert_eq!(reader2.open(&mut store2, NAME, 0x1234_5678, &mut na2, &mut rm), Status::Ok);
    assert_eq!(reader2.point(), None);

    // 3. Mid-point write failure latches failed; finish stays abort-only (never calls end_write).
    let mut store3 = MemStore::default();
    assert!(writer.begin_v4(&mut store3, NAME, 0x1234_5678, &wm));
    assert!(writer.on_page(&mut store3, &mut wm, &p0));
    store3.fail_after = Some(store3.temp.len() + 10);
    let mut write_cb3 = |bytes: &[u8]| store3.write(bytes);
    assert!(!writer.write_point(point_len, &mut write_cb3, &mut emit));
    assert!(writer.failed());
    assert!(!writer.finish(&mut store3, NAME, &wm));
    assert!(!store3.ended, "failed v4 finish must never call end_write");
    assert!(store3.published.is_none(), "failed v4 finish must never promote temp");
    assert!(store3.aborted);

    // 4. Short emitter on a completely healthy store: finish still stays abort-only
    //    and never promotes an index-less temp.
    let mut store4 = MemStore::default();
    assert!(writer.begin_v4(&mut store4, NAME, 0x1234_5678, &wm));
    assert!(writer.on_page(&mut store4, &mut wm, &p0));
    let mut write_cb4 = |bytes: &[u8]| store4.write(bytes);
    let mut short_emit = |crc_out: &mut CrcOut<'_>| crc_out.put(b"FIRP");
    assert!(!writer.write_point(point_len, &mut write_cb4, &mut short_emit));
    assert!(writer.failed());
    assert!(!writer.finish(&mut store4, NAME, &wm));
    assert!(!store4.ended, "short-emitter v4 finish must never call end_write");
    assert!(store4.published.is_none(), "short-emitter v4 finish must never promote temp");
    assert!(store4.aborted);
}

#[test]
fn point_cursor_binding_source_names_and_streaming_envelope_verify_and_reject_faults() {
    use newt_core::page_envelope::EnvelopeIo;

    // 1. Binding & Source decode roundtrip + exact length & bad magic rejection.
    let binding = Binding {
        book_digest: [0x5a; 32],
        spine: 7,
        generation: 0x0102_0304,
        segment_index: 4,
        page_count: 800,
        total_chars: 123_456,
        style_hash: 0xfeed_face,
        style_count: 9,
        style_issues: 1,
        style_state: 2,
        profile: 0,
        member_method: 8,
        member_offset: 4096,
        member_compressed: 22_000,
        member_uncompressed: 65_000,
        source_bytes: 999_999,
        pages_at: 510,
        cut: 32_100,
        fed: 32_240,
    };
    let mut raw_point = Vec::new();
    let payload_len = (BINDING_BYTES + SOURCE_BYTES + 1500) as u32;
    let crc = {
        let mut sink = |b: &[u8]| {
            raw_point.extend_from_slice(b);
            true
        };
        let mut out = CrcOut::new(payload_len, &mut sink);
        assert!(binding.encode(&mut out));
        assert!(encode_source(54_321, &mut out));
        assert!(out.put(&vec![0xab; 1500]));
        out.finish_crc()
    };
    raw_point.extend_from_slice(&crc.to_le_bytes());

    // Multi-window Stage 1 -> Stage 2 roundtrip across 1 KiB window boundary.
    let mut cursor = PointCursor::new(0, raw_point.len() as u32).unwrap();
    let mut win = [0u8; 1024];
    let (dec_binding, dec_src) = {
        let mut read = |off: u32, dst: &mut [u8]| {
            let at = off as usize;
            raw_point.get(at..at + dst.len()).map(|s| dst.copy_from_slice(s)).is_some()
        };
        let mut pin = cursor.bind(&mut win, &mut read);
        let b = Binding::decode(&mut pin.section(BINDING_BYTES as u32)).unwrap();
        let s = decode_source(&mut pin.section(SOURCE_BYTES as u32)).unwrap();
        (b, s)
    };
    assert_eq!(dec_binding, binding);
    assert_eq!(dec_src, 54_321);
    {
        let mut read = |off: u32, dst: &mut [u8]| {
            let at = off as usize;
            raw_point.get(at..at + dst.len()).map(|s| dst.copy_from_slice(s)).is_some()
        };
        let mut pin = cursor.bind(&mut win, &mut read);
        let mut tail = vec![0u8; 1500];
        let mut sec = pin.section(1500);
        assert!(sec.read_exact(&mut tail));
        assert!(sec.finish());
        assert!(tail.iter().all(|&b| b == 0xab));
        assert!(pin.finish_crc(crc));
    }

    // Pass B byte flip between Pass A and Pass B fails finish_crc.
    let mut mutated = raw_point.clone();
    mutated[BINDING_BYTES + SOURCE_BYTES + 1200] ^= 0x01;
    let mut cursor2 = PointCursor::new(0, mutated.len() as u32).unwrap();
    let mut read_mut = |off: u32, dst: &mut [u8]| {
        let at = off as usize;
        mutated.get(at..at + dst.len()).map(|s| dst.copy_from_slice(s)).is_some()
    };
    let mut pin2 = cursor2.bind(&mut win, &mut read_mut);
    assert!(Binding::decode(&mut pin2.section(BINDING_BYTES as u32)).is_some());
    assert!(decode_source(&mut pin2.section(SOURCE_BYTES as u32)).is_some());
    let mut tail2 = vec![0u8; 1500];
    assert!(pin2.section(1500).read_exact(&mut tail2));
    assert!(!pin2.finish_crc(crc), "Pass B CRC must detect a flipped byte");

    // 2. validate_xml_name accepts UTF-8 XML names and rejects injection/control bytes.
    assert!(validate_xml_name(b"html"));
    assert!(validate_xml_name("café".as_bytes()));
    assert!(validate_xml_name(b"svg:path"));
    for bad in [
        &b""[..],
        b"a>b",
        b"a<b",
        b"a b",
        b"a/b",
        b"a=b",
        b"a&b",
        b"a\0b",
        b"a\nb",
        &[0xff, 0xfe],
    ] {
        assert!(!validate_xml_name(bad), "name {bad:?} must be rejected");
    }

    // 3. Streaming 1 KiB envelope attach/range/exact verification and fault handling.
    const NAME: &[u8; 69] =
        b"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff.fibp";
    let payload: Vec<u8> = (0..(BLOCK_BYTES as usize * 2 + 777))
        .map(|i| ((i * 31 + 17) & 0xff) as u8)
        .collect();
    let route = Query::new(Some(NAME)).route;
    let mut dir = Vec::new();
    for (block, bytes) in payload.chunks(BLOCK_BYTES as usize).enumerate() {
        dir.extend_from_slice(&page_envelope::block_crc(&route, block as u32, bytes).to_le_bytes());
    }
    let dir_crc = page_envelope::crc(u32::MAX, &dir) ^ u32::MAX;
    let mut manifest = [0u8; 64];
    page_envelope::manifest(&mut manifest, payload.len() as u32, &route, dir_crc, 0);
    let env_bytes = [payload.as_slice(), &dir, &manifest].concat();

    struct MockIo {
        bytes: Vec<u8>,
        fail_poll_after: Option<usize>,
        polls: usize,
    }
    impl EnvelopeIo for MockIo {
        fn read(&mut self, _: &Query<'_>, off: u32, out: &mut [u8]) -> bool {
            let at = off as usize;
            self.bytes.get(at..at + out.len()).map(|s| out.copy_from_slice(s)).is_some()
        }
        fn poll(&mut self) -> bool {
            self.polls += 1;
            self.fail_poll_after.is_none_or(|limit| self.polls <= limit)
        }
    }

    let mut q = Query::new(Some(NAME));
    q.extent = env_bytes.len() as u32;
    q.bound = true;
    let mut io = MockIo { bytes: env_bytes.clone(), fail_poll_after: None, polls: 0 };
    assert!(page_envelope::stream_attach(&mut q, &mut win, &mut io));
    assert_eq!(q.payload, payload.len() as u32);
    // Cross-block verified exact read spanning blocks 0, 1, and 2.
    let start = BLOCK_BYTES - 123;
    let mut got = vec![0u8; BLOCK_BYTES as usize + 300];
    assert!(page_envelope::stream_verified_exact(&q, &mut win, start, &mut got, &mut io));
    assert_eq!(got, payload[start as usize..start as usize + got.len()]);

    // Corrupt byte in block 1 must fail stream_verified_exact.
    let mut bad_blk = env_bytes.clone();
    bad_blk[BLOCK_BYTES as usize + 50] ^= 0x80;
    let mut io_bad = MockIo { bytes: bad_blk, fail_poll_after: None, polls: 0 };
    assert!(!page_envelope::stream_verified_exact(&q, &mut win, start, &mut got, &mut io_bad));

    // Corrupt directory byte must fail stream_attach.
    let mut bad_dir = env_bytes.clone();
    bad_dir[payload.len()] ^= 1;
    let mut q_bad = Query::new(Some(NAME));
    q_bad.extent = bad_dir.len() as u32;
    q_bad.bound = true;
    let mut io_dir = MockIo { bytes: bad_dir, fail_poll_after: None, polls: 0 };
    assert!(!page_envelope::stream_attach(&mut q_bad, &mut win, &mut io_dir));

    // Poll cancellation stops immediately.
    let mut io_stop = MockIo { bytes: env_bytes, fail_poll_after: Some(2), polls: 0 };
    assert!(!page_envelope::stream_verified_exact(&q, &mut win, start, &mut got, &mut io_stop));

    // 4. EntityFilter::load_resume rejects impossible saved states:
    //    (a) source_done == 1 with unread raw tail,
    //    (b) source_done == 1 with in_entity == 1,
    //    (c) in_entity == 1 with non-empty pending bytes.
    let bad_filters: &[(&str, Vec<u8>)] = &[
        (
            "source_done with unread raw tail",
            vec![b'F', b'L', b'T', b'1', 0, 0, 0, 0, 1, 0, b'x', 1],
        ),
        (
            "source_done with in_entity",
            vec![b'F', b'L', b'T', b'1', 2, 1, 0, 0, 0, 0, b'n', b'b', 1],
        ),
        (
            "in_entity with pending bytes",
            vec![b'F', b'L', b'T', b'1', 1, 1, 1, 0, 0, 0, b'a', b'&', 0],
        ),
        (
            "empty pending with nonzero pending_pos",
            vec![b'F', b'L', b'T', b'1', 0, 0, 0, 1, 0, 0, 0],
        ),
        (
            "nonempty pending with exhausted pending_pos",
            vec![b'F', b'L', b'T', b'1', 0, 0, 1, 1, 0, 0, b'&', 0],
        ),
    ];
    for (label, bad_flt) in bad_filters {
        let flt_crc = page_envelope::crc(u32::MAX, bad_flt) ^ u32::MAX;
        let mut flt_framed = bad_flt.clone();
        flt_framed.extend_from_slice(&flt_crc.to_le_bytes());
        let mut fcur = PointCursor::new(0, flt_framed.len() as u32).unwrap();
        let mut fread = |off: u32, dst: &mut [u8]| {
            let at = off as usize;
            flt_framed.get(at..at + dst.len()).map(|s| dst.copy_from_slice(s)).is_some()
        };
        let mut fpin = fcur.bind(&mut win, &mut fread);
        let mut ef = newt_core::xml_input::EntityFilter::new();
        assert!(
            !ef.load_resume(&mut fpin.section(bad_flt.len() as u32)),
            "EntityFilter must reject {label}"
        );
    }
}
