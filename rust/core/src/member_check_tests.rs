use super::*;
use crate::zip_container::Catalog;
use crate::zip_window::arena::Arena;
extern crate std;
use std::vec::Vec;

struct Bytes {
    data: Vec<u8>,
    fail_at: Option<u64>,
}
impl BookSource for Bytes {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if self.fail_at == Some(at) {
            return -1;
        }
        let rest = self.data.get(at as usize..).unwrap_or(&[]);
        let n = rest.len().min(out.len());
        out[..n].copy_from_slice(&rest[..n]);
        n as i32
    }
}

/// One stored member. `local` edits the local header bytes after they are built.
struct Member<'a> {
    name: &'a [u8],
    local_name: &'a [u8],
    flags: u16,
    local_flags: u16,
    local_method: u16,
    extra: u16,
}
impl Default for Member<'_> {
    fn default() -> Self {
        Self { name: b"OEBPS/c.xhtml", local_name: b"OEBPS/c.xhtml", flags: 0, local_flags: 0, local_method: 0, extra: 0 }
    }
}
const BODY: &[u8] = b"<html/>";

fn archive(m: &Member<'_>) -> Vec<u8> {
    let mut z = Vec::new();
    z.extend(0x04034b50u32.to_le_bytes());
    z.extend(20u16.to_le_bytes());
    z.extend(m.local_flags.to_le_bytes());
    z.extend(m.local_method.to_le_bytes());
    z.extend([0; 8]); // time, date, crc
    z.extend((BODY.len() as u32).to_le_bytes());
    z.extend((BODY.len() as u32).to_le_bytes());
    z.extend((m.local_name.len() as u16).to_le_bytes());
    z.extend(m.extra.to_le_bytes());
    z.extend(m.local_name);
    z.extend(BODY);
    let directory = z.len() as u32;
    z.extend(0x02014b50u32.to_le_bytes());
    z.extend([20, 0, 20, 0]);
    z.extend(m.flags.to_le_bytes());
    z.extend(0u16.to_le_bytes());
    z.extend([0; 8]);
    z.extend((BODY.len() as u32).to_le_bytes());
    z.extend((BODY.len() as u32).to_le_bytes());
    z.extend((m.name.len() as u16).to_le_bytes());
    z.extend([0; 12]); // extra, comment, disk, internal, external attributes
    z.extend(0u32.to_le_bytes());
    z.extend(m.name);
    let size = z.len() as u32 - directory;
    z.extend(0x06054b50u32.to_le_bytes());
    z.extend([0; 4]);
    z.extend(1u16.to_le_bytes());
    z.extend(1u16.to_le_bytes());
    z.extend(size.to_le_bytes());
    z.extend(directory.to_le_bytes());
    z.extend([0; 2]);
    z
}

/// Catalog the archive, then run the open-time check on its one member.
fn checked(m: &Member<'_>, fail_at: Option<u64>) -> Result<(Checked, u64), Status> {
    let mut source = Bytes { data: archive(m), fail_at: None };
    let mut storage = [0; 2048];
    let mut arena = Arena::new(&mut storage);
    let mut entries = [Entry::default(); 1];
    let mut catalog = Catalog::new(&mut source, &mut arena, &mut entries);
    assert_eq!(catalog.open(&mut [0; 4099], &mut [0; 1024]), Status::Ok);
    let entry = *catalog.entry(0).unwrap();
    let name: Vec<u8> = catalog.name(&entry).unwrap().into();
    let directory = catalog.directory_offset();
    source.fail_at = fail_at;
    check(&mut source, &entry, &name, directory)
}

#[test]
fn matching_member_is_checked_without_its_arena_name() {
    let (ok, data) = checked(&Member::default(), None).unwrap();
    assert_eq!(ok.name, None);
    assert_eq!(data, 30 + 13);
    assert_eq!(ok.uncompressed_size, BODY.len() as u32);
}

#[test]
fn local_header_disagreement_is_damaged() {
    let long_a = [b'a'; 100];
    let mut long_b = [b'a'; 100];
    long_b[90] = b'b';
    for (why, m) in [
        ("flags", Member { local_flags: 8, ..Member::default() }),
        ("method", Member { local_method: 8, ..Member::default() }),
        ("name length", Member { local_name: b"OEBPS/c.xhtm", ..Member::default() }),
        ("name bytes", Member { local_name: b"OEBPS/C.xhtml", ..Member::default() }),
        ("NUL in local name", Member { local_name: b"OEBPS/c\0xhtml", ..Member::default() }),
        ("second name chunk", Member { name: &long_a, local_name: &long_b, ..Member::default() }),
    ] {
        assert_eq!(checked(&m, None), Err(Status::Damaged), "{why}");
    }
    let mut bad_signature = archive(&Member::default());
    bad_signature[0] = b'X';
    let mut source = Bytes { data: bad_signature, fail_at: None };
    let entry = Entry { local_header_offset: 0, compressed_size: 7, uncompressed_size: 7, ..Entry::default() };
    let directory = (30 + 13 + BODY.len()) as u32;
    assert_eq!(check(&mut source, &entry, b"OEBPS/c.xhtml", directory), Err(Status::Damaged));
}

#[test]
fn data_must_end_before_the_directory() {
    // A local extra field moves the data start past the central directory.
    assert_eq!(checked(&Member { extra: 4, ..Member::default() }, None), Err(Status::Truncated));
}

#[test]
fn encrypted_member_keeps_its_admission_status() {
    let m = Member { flags: 1, local_flags: 1, ..Member::default() };
    assert_eq!(checked(&m, None), Err(Status::Encrypted));
}

#[test]
fn unreadable_header_or_name_is_an_io_error() {
    assert_eq!(checked(&Member::default(), Some(0)), Err(Status::IoError));
    assert_eq!(checked(&Member::default(), Some(30)), Err(Status::IoError));
}
