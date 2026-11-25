use newt_core::member_check::Checked;
use newt_core::{
    zip_container::*,
    zip_window::{arena::Arena, source::BookSource},
};
struct Source {
    bytes: Vec<u8>,
    max: usize,
    fail: Option<u64>,
    trace: Vec<(u64, usize, i32)>,
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let n = if self.fail == Some(at) {
            -1
        } else {
            let at = usize::try_from(at).unwrap_or(usize::MAX);
            let bytes = self.bytes.get(at..).unwrap_or(&[]);
            let n = bytes.len().min(out.len()).min(self.max);
            out[..n].copy_from_slice(&bytes[..n]);
            n as i32
        };
        self.trace.push((at, out.len(), n));
        n
    }
}
fn source(bytes: Vec<u8>) -> Source {
    Source {
        bytes,
        max: usize::MAX,
        fail: None,
        trace: vec![],
    }
}
fn eocd(count: u16, offset: u32, size: u32) -> Vec<u8> {
    let mut b = vec![0; 22];
    b[..4].copy_from_slice(&0x06054b50u32.to_le_bytes());
    b[10..12].copy_from_slice(&count.to_le_bytes());
    b[12..16].copy_from_slice(&size.to_le_bytes());
    b[16..20].copy_from_slice(&offset.to_le_bytes());
    b
}
#[test]
fn hash_is_c_string_wrapping_fnv() {
    assert_eq!(hash_path(b""), 2166136261);
    assert_eq!(hash_path(b"a"), 0xe40c292c);
    assert_eq!(hash_path(b"a\0b"), hash_path(b"a"));
}
#[test]
fn scan_overlap_short_reads_and_unchanged_failure_outputs() {
    for comment in [0, 4073, 4074, 4075, 65535] {
        let mut bytes = eocd(0, 0, 0);
        bytes.resize(22 + comment, 0x71);
        let mut s = source(bytes);
        s.max = 7;
        let mut scan = [0; 4099];
        let mut d = Directory {
            entries: 42,
            offset: 42,
            size: 42,
        };
        assert_eq!(locate(&mut s, &mut scan, &mut d), Status::Ok);
        assert_eq!(d, Directory::default());
        s.fail = Some((s.bytes.len() as u64).saturating_sub(4096));
        d.entries = 42;
        assert_eq!(locate(&mut s, &mut scan, &mut d), Status::IoError);
        assert_eq!(d.entries, 42);
    }
}
#[test]
fn last_signature_wins_even_if_too_close_to_eof() {
    let mut bytes = eocd(0, 0, 0);
    bytes.extend_from_slice(&0x06054b50u32.to_le_bytes());
    assert_eq!(
        locate(
            &mut source(bytes),
            &mut [0; 4099],
            &mut Directory::default()
        ),
        Status::NotZip
    );
}
#[test]
fn unsupported_and_range_statuses() {
    for (b, status) in [
        (vec![], Status::NotZip),
        (eocd(u16::MAX, 0, 0), Status::Unsupported),
        (eocd(0, u32::MAX, 0), Status::Unsupported),
        (eocd(0, 22, 1), Status::Truncated),
    ] {
        assert_eq!(
            locate(&mut source(b), &mut [0; 4099], &mut Directory::default()),
            status
        );
    }
}
#[test]
fn raw_short_reads_zero_length_and_failed_reopen_reset() {
    let mut s = source(b"abcdef".to_vec());
    s.max = 2;
    let e = Entry::raw(6);
    let bad = Entry {
        method: 8,
        ..Entry::raw(1)
    };
    let mut r = Reader::new(&mut s, None);
    let mut out = [0; 8];
    assert_eq!(r.read(&mut out), -1);
    let e = Checked::unverified(e);
    assert_eq!(r.open(&e), Status::Ok);
    assert_eq!(r.read(&mut []), 0);
    assert_eq!(r.read(&mut out), 2);
    assert_eq!(&out[..2], b"ab");
    assert_eq!(r.total_produced(), 2);
    let bad = Checked::unverified(bad);
    assert_eq!(r.open(&bad), Status::Unsupported);
    assert_eq!(r.total_produced(), 0);
    assert_eq!(r.entry(), Some(&*bad));
}
#[test]
fn catalog_failure_unpublishes_old_entries_and_keeps_consumed_name_storage() {
    let mut bytes = vec![0; 46];
    bytes[..4].copy_from_slice(&0x02014b50u32.to_le_bytes());
    bytes[28] = 1;
    bytes.push(b'x');
    bytes.extend(eocd(1, 0, 47));
    let mut s = source(bytes);
    let mut names = [0; 4];
    let mut arena = Arena::new(&mut names);
    let mut entries = [Entry::default(); 1];
    let mut c = Catalog::new(&mut s, &mut arena, &mut entries);
    assert_eq!(c.open(&mut [0; 4099], &mut [0; 1024]), Status::Ok);
    assert_eq!(c.entry_count(), 1);
    assert!(c.find(Some(b"x")).is_some());
    assert!(c.find(None).is_none());
    assert_eq!(c.arena_used(), 2);
    assert_eq!(c.open(&mut [0; 4099], &mut [0; 1024]), Status::Ok);
    assert_eq!(c.arena_used(), 4);
    assert_eq!(c.open(&mut [0; 4099], &mut [0; 1024]), Status::OutOfMemory);
    assert_eq!(c.entry_count(), 0);
    assert!(c.find_by_hash(hash_path(b"x")).is_none());
}
fn deflate_source(raw: &[u8]) -> Source {
    let mut bytes = vec![0; 30];
    bytes[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
    bytes.extend_from_slice(raw);
    source(bytes)
}
#[test]
fn borrowed_deflate_stored_block_pending_delivery_and_reopen() {
    use newt_core::inflate::{BitBuffer, State};
    // Raw DEFLATE final stored block, LEN=6, NLEN=!6.
    let mut s = deflate_source(b"\x01\x06\x00\xf9\xffabcdef");
    s.max = 2;
    let e = Entry {
        compressed_size: 11,
        uncompressed_size: 1,
        method: 8,
        ..Entry::default()
    };
    let mut state = Box::new(State::new(BitBuffer::Bits64));
    let mut dict = vec![0; 32768];
    let mut input = vec![0; 2048];
    let mut r = Reader::new(
        &mut s,
        Some(InflateScratch {
            state: &mut state,
            dictionary: dict.as_mut_slice().try_into().unwrap(),
            input: input.as_mut_slice().try_into().unwrap(),
        }),
    );
    let mut out = [0; 4];
    let e = Checked::unverified(e);
    assert_eq!(r.open(&e), Status::Ok);
    assert_eq!(r.read(&mut out[..1]), 1);
    assert_eq!(out[0], b'a');
    assert_eq!(r.total_produced(), 1);
    assert_eq!(r.read(&mut out), 4);
    assert_eq!(&out, b"bcde");
    assert_eq!(r.read(&mut out), 1);
    assert_eq!(out[0], b'f');
    assert_eq!(r.read(&mut out), 0);
    assert_eq!(r.total_produced(), 6); // Central uncompressed size is not a deflate cap.
    assert_eq!(r.open(&e), Status::Ok);
    assert_eq!(r.total_produced(), 0);
    assert_eq!(r.read(&mut out), 4);
    assert_eq!(&out, b"abcd");
}
#[test]
fn deflate_truncation_commits_no_produced_count_on_failed_call() {
    use newt_core::inflate::{BitBuffer, State};
    let mut s = deflate_source(b"\x01\x06\x00\xf9\xffabc");
    let e = Entry {
        compressed_size: 8,
        uncompressed_size: 6,
        method: 8,
        ..Entry::default()
    };
    let mut state = Box::new(State::new(BitBuffer::Bits64));
    let mut dict = vec![0; 32768];
    let mut input = vec![0; 2048];
    let mut r = Reader::new(
        &mut s,
        Some(InflateScratch {
            state: &mut state,
            dictionary: dict.as_mut_slice().try_into().unwrap(),
            input: input.as_mut_slice().try_into().unwrap(),
        }),
    );
    let e = Checked::unverified(e);
    assert_eq!(r.open(&e), Status::Ok);
    let mut out = [0xa5; 8];
    assert_eq!(r.read(&mut out), -1);
    assert_eq!(r.total_produced(), 0);
    // Negative inflate status does not discard decoded pending output.
    assert_eq!(r.read(&mut out[..3]), 3);
    assert_eq!(&out[..3], b"abc");
    assert_eq!(r.total_produced(), 3);
}
