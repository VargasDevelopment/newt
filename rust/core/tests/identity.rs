use newt_core::durable::{Backend, BlobRead, Codec, Commit, DurableWrite, Load, StorageOpen};
use newt_core::library_name::{FILENAME_BYTES, valid_library_book_name};
use newt_core::sd_resume::SdResumeRecord;
use newt_core::selected_book::{SelectedBookCodec, SelectedBookStore};
use newt_core::sleep_intent::{SleepResumeCodec, SleepResumeIntent, SleepResumeStore};

fn identity(name: &[u8], prefix: u8, tail: u8) -> SdResumeRecord {
    let mut digest = [0; 32];
    digest[0] = prefix;
    digest[31] = tail;
    SdResumeRecord::new(core::ffi::CStr::from_bytes_until_nul(name).unwrap(), digest).unwrap()
}

fn selected(value: SdResumeRecord, generation: u32) -> Vec<u8> {
    let mut bytes = [0; 144];
    assert!(SelectedBookCodec::encode(value, generation, &mut bytes));
    bytes.to_vec()
}

fn sleep(value: SleepResumeIntent, generation: u32) -> Vec<u8> {
    let mut bytes = [0; 32];
    assert!(SleepResumeCodec::encode(value, generation, &mut bytes));
    bytes.to_vec()
}

#[derive(Default)]
struct Memory {
    slots: [Option<Vec<u8>>; 2],
    writes: usize,
}

impl Backend for Memory {
    fn begin(&mut self, _: &str, _: bool) -> StorageOpen {
        StorageOpen::Opened
    }
    fn end(&mut self) {}
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        let index = usize::from(key == "slot1");
        let Some(bytes) = &self.slots[index] else {
            return BlobRead::Missing;
        };
        let count = bytes.len().min(output.len());
        output[..count].copy_from_slice(&bytes[..count]);
        BlobRead::Present(bytes.len())
    }
    fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.writes += 1;
        self.slots[usize::from(key == "slot1")] = Some(bytes.to_vec());
        bytes.len()
    }
    fn readback_durable(&self) -> bool {
        true
    }
}

#[test]
fn legacy_record_name_validation_is_bounded_scalar_utf8() {
    for valid in [
        &b"a.EpUb\0"[..],
        "\u{10ffff}.epub\0".as_bytes(),
        "\u{a0}\u{feff}.epub\0".as_bytes(),
        b":Newt:..:odd.epub\0", // retained validator, not catalog authorization
        b"a.epub\0ignored\\tail",
    ] {
        assert!(valid_library_book_name(valid), "{valid:?}");
    }
    for invalid in [
        &b"a.epub"[..],
        b".epub\0",
        b".a.epub\0",
        b"a/b.epub\0",
        b"a\\b.epub\0",
        b"a\x7f.epub\0",
        b"\xc2\x9f.epub\0",
        b"\xc0\xaf.epub\0",
        b"\xed\xa0\x80.epub\0",
        b"\xf4\x90\x80\x80.epub\0",
        b"\xf0\x9f.epub\0",
    ] {
        assert!(!valid_library_book_name(invalid), "{invalid:?}");
    }
    let mut limit = [b'x'; FILENAME_BYTES];
    limit[90..95].copy_from_slice(b".epub");
    limit[95] = 0;
    assert!(valid_library_book_name(&limit));
    limit[95] = b'x';
    assert!(!valid_library_book_name(&limit));
}

#[test]
fn retained_padding_can_be_valid_but_selected_wire_is_canonical() {
    let canonical = identity(b":Shelf:Title.epub\0", 0, 1);
    assert_eq!(canonical.book_id(), 0); // full SHA is nonzero; prefix may be zero
    let mut padded = canonical;
    padded.filename[95] = 0x80;
    padded.checksum = padded.calculate_checksum();
    assert!(padded.valid());
    assert_eq!(selected(padded, 9), selected(canonical, 9));
    let bytes = selected(padded, 9);
    assert_eq!(SelectedBookCodec::decode(&bytes), Some(canonical));

    let mut dirty_wire = bytes;
    dirty_wire[139] = 1;
    let checksum = newt_core::wire::fnv1a(&dirty_wire[..140]);
    dirty_wire[140..].copy_from_slice(&checksum.to_le_bytes());
    assert!(SelectedBookCodec::known(&dirty_wire));
    assert!(SelectedBookCodec::decode(&dirty_wire).is_none());
}

#[test]
fn short_id_collision_cannot_protect_a_different_complete_sha() {
    let a = identity(b"Alpha.epub\0", 7, 1);
    let collision = identity(b"Beta.epub\0", 7, 2);
    let mut memory = Memory {
        slots: [Some(selected(a, 10)), Some(selected(collision, 11))],
        ..Memory::default()
    };
    let before = memory.slots.clone();
    let mut store = SelectedBookStore::new(&mut memory);
    assert_eq!(store.load_book(a.book_id()), Load::Ambiguous);
    assert_eq!(store.save_protected(a, a.book_id()), Commit::Unconfirmed);
    assert_eq!(memory.slots, before);
    assert_eq!(memory.writes, 0);
}

#[test]
fn protected_old_selection_survives_newer_selection_replacement() {
    let a = identity(b"Alpha.epub\0", 1, 1);
    let b = identity(b"Beta.epub\0", 2, 2);
    let c = identity(b"Gamma.epub\0", 3, 3);
    let old = selected(a, 10);
    let mut memory = Memory {
        slots: [Some(old.clone()), Some(selected(b, 11))],
        ..Memory::default()
    };
    assert_eq!(
        SelectedBookStore::new(&mut memory).save_protected(c, a.book_id()),
        Commit::Saved
    );
    assert_eq!(memory.slots[0], Some(old));
    assert_eq!(
        SelectedBookStore::new(&mut memory).load_book(a.book_id()),
        Load::Loaded(a)
    );
    assert_eq!(SelectedBookStore::new(&mut memory).load(), Load::Loaded(c));
}

#[test]
fn sleep_settings_preserve_discriminator_anchor_and_cadence() {
    for cadence in [1, 5, 10] {
        for low in 0..16 {
            let intent = SleepResumeIntent {
                book: u64::MAX,
                checkpoint: 0, // zero checkpoint is allowed for nonzero books
                settings: (cadence << 8) | low,
            };
            assert_eq!(intent.valid(), low & 4 != 0);
            if intent.valid() {
                assert_eq!(SleepResumeCodec::decode(&sleep(intent, 0)), Some(intent));
            }
        }
    }
    for settings in [0, 4, 0x204, 0x514, 0x10504, 0xff04] {
        assert!(
            !SleepResumeIntent {
                book: 1,
                checkpoint: 1,
                settings,
            }
            .valid()
        );
    }
    assert!(SleepResumeIntent::default().valid());
    assert!(
        !SleepResumeIntent {
            book: 0,
            checkpoint: 1,
            settings: 0,
        }
        .valid()
    );
}

#[test]
fn corrupt_newer_home_does_not_resurrect_reading_but_explicit_save_repairs() {
    let reading = SleepResumeIntent {
        book: 1,
        checkpoint: 0x12345678,
        settings: 0x50f,
    };
    let mut torn_home = sleep(SleepResumeIntent::default(), 8);
    torn_home[28] ^= 1;
    for swap in [false, true] {
        let intact = sleep(reading, 7);
        let mut slots = [Some(intact.clone()), Some(torn_home.clone())];
        if swap {
            slots.swap(0, 1);
        }
        let mut memory = Memory {
            slots,
            ..Memory::default()
        };
        assert_eq!(SleepResumeStore::new(&mut memory).load(), Load::Failed);
        assert_eq!(memory.writes, 0);
        assert_eq!(
            SleepResumeStore::new(&mut memory).save(SleepResumeIntent::default()),
            Commit::Saved
        );
        assert_eq!(memory.slots[usize::from(swap)], Some(intact));
        assert_eq!(memory.writes, 1);
        assert_eq!(
            SleepResumeStore::new(&mut memory).load(),
            Load::Loaded(SleepResumeIntent::default())
        );
    }
}
