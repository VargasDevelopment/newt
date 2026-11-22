//! NWLS v1 selected-book wire records and protected SHA peer selection (MIT).
use crate::durable::{Admission, Backend, Codec, Commit, Load, RecordStore};
use crate::library_name::{name_bytes, valid_library_book_name};
use crate::sd_resume::SdResumeRecord;
use crate::wire::{fnv1a, get32, put32};

pub const BYTES: usize = 144;
pub struct SelectedBookCodec;

impl Codec<BYTES> for SelectedBookCodec {
    type Value = SdResumeRecord;
    const NAMESPACE: &'static str = "newt_selected";

    fn known(bytes: &[u8]) -> bool {
        bytes.len() == BYTES && get32(bytes, 0) == 0x534c_574e && get32(bytes, 4) == 1
    }

    fn decode(bytes: &[u8]) -> Option<SdResumeRecord> {
        if !Self::known(bytes) || get32(bytes, 140) != fnv1a(&bytes[..140]) {
            return None;
        }
        let name = &bytes[44..140];
        if !valid_library_book_name(name) {
            return None;
        }
        let length = name_bytes(name)?.len();
        if name[length + 1..].iter().any(|&byte| byte != 0) {
            return None;
        }
        SdResumeRecord::new(
            core::ffi::CStr::from_bytes_until_nul(name).ok()?,
            bytes[12..44].try_into().ok()?,
        )
    }

    fn encode(value: SdResumeRecord, sequence: u32, bytes: &mut [u8; BYTES]) -> bool {
        if !value.valid() {
            return false;
        }
        let length = name_bytes(&value.filename).expect("validated name").len();
        bytes.fill(0);
        put32(bytes, 0, 0x534c_574e);
        put32(bytes, 4, 1);
        put32(bytes, 8, sequence);
        bytes[12..44].copy_from_slice(&value.digest);
        bytes[44..44 + length + 1].copy_from_slice(&value.filename[..length + 1]);
        let checksum = fnv1a(&bytes[..140]);
        put32(bytes, 140, checksum);
        true
    }
}

pub struct SelectedBookStore<'a, B: Backend> {
    records: RecordStore<'a, B, SelectedBookCodec, BYTES>,
}

impl<'a, B: Backend> SelectedBookStore<'a, B> {
    pub fn new(backend: &'a mut B) -> Self {
        Self {
            records: RecordStore::new(backend),
        }
    }

    pub fn load(&mut self) -> Load<SdResumeRecord> {
        self.records.load()
    }

    pub fn save(&mut self, value: SdResumeRecord) -> Commit {
        self.records.save(value)
    }

    pub fn save_admitted(
        &mut self,
        value: SdResumeRecord,
        admission: &mut Admission<'_>,
    ) -> Commit {
        self.records.save_admitted(value, admission)
    }

    pub fn load_book(&mut self, book: u64) -> Load<SdResumeRecord> {
        self.records.load_matching(
            |candidate| candidate.book_id() == book,
            |a, b| a.digest == b.digest,
        )
    }

    pub fn save_protected(&mut self, value: SdResumeRecord, protected_book: u64) -> Commit {
        if protected_book == 0 {
            return self.save(value);
        }
        let protected = match self.load_book(protected_book) {
            Load::Empty => return self.save(value),
            Load::Loaded(protected) => protected,
            _ => return Commit::Unconfirmed,
        };
        if value.book_id() == protected_book && value.digest != protected.digest {
            return Commit::Unconfirmed;
        }
        self.records.save_protected(value, protected)
    }
}
