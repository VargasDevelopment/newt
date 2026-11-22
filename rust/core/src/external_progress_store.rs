// SPDX-License-Identifier: MIT
//! Per-book durable reading progress store bound to the book's 256-bit digest.
use crate::durable::{
    Admission, Backend, BlobRead, Commit, DurableWrite, Load, SLOT_KEYS, StorageOpen, commit_bytes,
};
use crate::progress::{BYTES, Progress, Selection, select};
use crate::progress_store::{PeerLocators, ProgressStore};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Status {
    Ready,
    Empty,
    Loaded,
    Saved,
    Ambiguous,
    UnsupportedIdentity,
    IdentityMismatch,
    DigestMismatch,
    MalformedDigest,
    InvalidRecord,
    StorageFailure,
    WriteFailure,
    VerifyFailure,
}

pub struct ExternalProgressStore<'a, B: Backend> {
    store: &'a mut B,
    digest: [u8; 32],
    namespace: [u8; 15],
    book: u64,
    status: Status,
}
impl<'a, B: Backend> ExternalProgressStore<'a, B> {
    pub fn new(store: &'a mut B, digest: Option<&[u8; 32]>) -> Self {
        let mut owner = Self {
            store,
            digest: [0; 32],
            namespace: [0; 15],
            book: 0,
            status: Status::Ready,
        };
        let Some(digest) = digest else {
            owner.status = Status::UnsupportedIdentity;
            return owner;
        };
        owner.digest = *digest;
        owner.book = u64::from_le_bytes(digest[..8].try_into().expect("eight digest bytes"));
        owner.namespace[..2].copy_from_slice(b"nb");
        for i in 0..13 {
            owner.namespace[i + 2] = b"0123456789abcdef"
                [usize::from((digest[i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 15)];
        }
        if owner.book == 0 {
            owner.status = Status::UnsupportedIdentity;
        }
        owner
    }
    pub fn book_id(&self) -> u64 {
        self.book
    }
    /// Empty string when no digest is bound; a zero book still has a route.
    pub fn storage_namespace(&self) -> &str {
        if self.namespace[0] == 0 {
            ""
        } else {
            core::str::from_utf8(&self.namespace).expect("hex namespace")
        }
    }
    pub fn status(&self) -> Status {
        self.status
    }
    fn supported(&mut self) -> bool {
        if self.book == 0 {
            self.status = Status::UnsupportedIdentity;
            false
        } else {
            self.status = Status::Ready;
            true
        }
    }
    pub fn load(&mut self, out: &mut Progress) -> Load<()> {
        if !self.supported() {
            return Load::Failed;
        }
        let namespace = self.namespace;
        let mut checked = Checked::new(self, None);
        let result = ProgressStore::in_namespace(
            &mut checked,
            core::str::from_utf8(&namespace).expect("hex"),
        )
        .load();
        match result {
            Load::Loaded(value) => {
                *out = value;
                self.status = Status::Loaded;
                Load::Loaded(())
            }
            Load::Empty => {
                self.status = Status::Empty;
                Load::Empty
            }
            Load::Ambiguous => {
                self.status = Status::Ambiguous;
                Load::Ambiguous
            }
            Load::Failed => {
                if self.status == Status::Ready {
                    self.status = Status::StorageFailure;
                }
                if self.status == Status::Ambiguous {
                    Load::Ambiguous
                } else {
                    Load::Failed
                }
            }
        }
    }
    pub fn load_checkpoint(&mut self, out: &mut Progress, checkpoint: u32) -> Load<()> {
        if !self.supported() {
            return Load::Failed;
        }
        let namespace = self.namespace;
        let mut checked = Checked::new(self, None);
        let result = ProgressStore::in_namespace(
            &mut checked,
            core::str::from_utf8(&namespace).expect("hex"),
        )
        .load_checkpoint(checkpoint);
        if let Load::Loaded(value) = result {
            *out = value;
            self.status = Status::Loaded;
            return Load::Loaded(());
        }
        if self.status == Status::Ready {
            self.status = Status::InvalidRecord;
        }
        match result {
            Load::Empty => Load::Empty,
            Load::Ambiguous => Load::Ambiguous,
            _ => Load::Failed,
        }
    }
    pub fn load_peer_locators(&mut self, peers: &mut PeerLocators) -> Load<()> {
        if !self.supported() {
            return Load::Failed;
        }
        let namespace = self.namespace;
        let mut candidate = *peers;
        let mut checked = Checked::new(self, None);
        let result = ProgressStore::in_namespace(
            &mut checked,
            core::str::from_utf8(&namespace).expect("hex"),
        )
        .load_peer_locators(&mut candidate);
        match result {
            Load::Loaded(()) => {
                *peers = candidate;
                self.status = Status::Loaded;
            }
            Load::Empty => {
                *peers = PeerLocators::default();
                self.status = Status::Empty;
            }
            Load::Ambiguous => self.status = Status::Ambiguous,
            Load::Failed => {
                if self.status == Status::Ready {
                    self.status = Status::StorageFailure;
                }
            }
        }
        result
    }
    pub fn save(
        &mut self,
        value: &mut Progress,
        peers: Option<&mut PeerLocators>,
        protected: Option<u32>,
        strict: bool,
        admission: Option<&mut Admission<'_>>,
    ) -> Commit {
        if !self.supported() {
            return Commit::Unconfirmed;
        }
        if value.book != self.book {
            self.status = Status::IdentityMismatch;
            return Commit::Unconfirmed;
        }
        let namespace = self.namespace;
        let mut checked = Checked::new(self, admission);
        let mut progress = ProgressStore::in_namespace(
            &mut checked,
            core::str::from_utf8(&namespace).expect("hex"),
        );
        let result = match peers {
            Some(peers) => progress.save_with_peers(value, peers, protected, strict),
            None => progress.save(value, protected, strict),
        };
        if result == Commit::Saved {
            self.status = Status::Saved;
        } else if self.status == Status::Ready {
            self.status = Status::WriteFailure;
        }
        result
    }
}

// No slots, record copies, self references, raw pointers, or heap. Opening borrows
// ProgressStore's 104 bytes once; commit verification borrows the intended bytes.
struct Checked<'o, 'a, 'g, 'q, B: Backend> {
    owner: &'o mut ExternalProgressStore<'a, B>,
    admission: Option<&'g mut Admission<'q>>,
    opened: bool,
}
impl<'o, 'a, 'g, 'q, B: Backend> Checked<'o, 'a, 'g, 'q, B> {
    fn new(
        owner: &'o mut ExternalProgressStore<'a, B>,
        admission: Option<&'g mut Admission<'q>>,
    ) -> Self {
        Self {
            owner,
            admission,
            opened: false,
        }
    }
    fn fail(&mut self, status: Status) -> (StorageOpen, Option<[BlobRead; 2]>) {
        self.end();
        self.owner.status = status;
        (StorageOpen::Failed, None)
    }
}
impl<B: Backend> Backend for Checked<'_, '_, '_, '_, B> {
    fn begin(&mut self, _: &str, _: bool) -> StorageOpen {
        self.owner.status = Status::StorageFailure;
        StorageOpen::Failed
    }
    fn begin_progress(
        &mut self,
        namespace: &str,
        readonly: bool,
        slots: &mut [[u8; BYTES]; 2],
    ) -> (StorageOpen, Option<[BlobRead; 2]>) {
        if namespace != self.owner.storage_namespace() {
            return self.fail(Status::StorageFailure);
        }
        if !readonly && self.admission.as_mut().is_some_and(|gate| !gate.enter()) {
            return (StorageOpen::Failed, None);
        }
        let opened = self.owner.store.begin(namespace, readonly);
        if opened == StorageOpen::Missing && readonly {
            self.owner.status = Status::Empty;
            return (StorageOpen::Missing, None);
        }
        if opened != StorageOpen::Opened {
            return self.fail(Status::StorageFailure);
        }
        self.opened = true;
        let mut observed_digest = [0; 32];
        let digest_read = self.owner.store.read_blob("digest", &mut observed_digest);
        match digest_read {
            BlobRead::Failed => return self.fail(Status::StorageFailure),
            BlobRead::Present(n) => {
                if n != 32 {
                    return self.fail(Status::MalformedDigest);
                }
                if observed_digest != self.owner.digest {
                    return self.fail(Status::DigestMismatch);
                }
            }
            BlobRead::Missing => {}
        }
        let mut reads = [BlobRead::Missing; 2];
        let mut any_present = false;
        let mut any_valid = false;
        for i in 0..2 {
            reads[i] = self.owner.store.read_blob(SLOT_KEYS[i], &mut slots[i]);
            match reads[i] {
                BlobRead::Failed => return self.fail(Status::StorageFailure),
                BlobRead::Present(n) => {
                    any_present = true;
                    if n == BYTES
                        && let Some(decoded) = Progress::decode(&slots[i])
                    {
                        if decoded.book != self.owner.book {
                            return self.fail(Status::IdentityMismatch);
                        }
                        any_valid = true;
                    }
                }
                BlobRead::Missing => {}
            }
        }
        if digest_read == BlobRead::Missing {
            if any_present {
                return self.fail(Status::InvalidRecord);
            }
            if readonly {
                self.end();
                self.owner.status = Status::Empty;
                return (StorageOpen::Missing, None);
            }
            let digest = self.owner.digest;
            if commit_bytes(self, "digest", &digest, &mut observed_digest) != Commit::Saved {
                let status = if self.owner.status == Status::Ready {
                    Status::WriteFailure
                } else {
                    self.owner.status
                };
                return self.fail(status);
            }
        } else if any_present && !any_valid {
            return self.fail(Status::InvalidRecord);
        }
        let slot = |i: usize| {
            if reads[i] == BlobRead::Present(BYTES) {
                &slots[i][..]
            } else {
                &slots[i][..0]
            }
        };
        if select(slot(0), slot(1)) == Selection::Ambiguous {
            return self.fail(Status::Ambiguous);
        }
        (StorageOpen::Opened, Some(reads))
    }
    fn end(&mut self) {
        if self.opened {
            self.owner.store.end();
        }
        self.opened = false;
    }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        if !self.opened {
            return BlobRead::Failed;
        }
        let result = self.owner.store.read_blob(key, output);
        if result == BlobRead::Failed {
            self.owner.status = Status::StorageFailure;
        }
        if result == BlobRead::Present(BYTES)
            && output.len() >= BYTES
            && let Some(value) = Progress::decode(&output[..BYTES])
            && value.book != self.owner.book
        {
            self.owner.status = Status::IdentityMismatch;
            return BlobRead::Failed;
        }
        result
    }
    fn readback_bytes(&mut self, key: &str, expected: &[u8], output: &mut [u8]) -> BlobRead {
        // Physical I/O failure becomes VerifyFailure;
        // byte mismatch precedes interpretation as a valid wrong-book record.
        if !self.opened {
            return BlobRead::Failed;
        }
        let result = self.owner.store.read_blob(key, output);
        if result == BlobRead::Failed {
            self.owner.status = Status::StorageFailure;
        }
        if result != BlobRead::Present(expected.len()) || output != expected {
            self.owner.status = Status::VerifyFailure;
            return BlobRead::Failed;
        }
        if result == BlobRead::Present(BYTES)
            && let Some(value) = Progress::decode(output)
            && value.book != self.owner.book
        {
            self.owner.status = Status::IdentityMismatch;
            return BlobRead::Failed;
        }
        result
    }
    fn put_bytes(&mut self, permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        if !self.opened {
            return 0;
        }
        let written = self.owner.store.put_bytes(permit, key, bytes);
        if written != bytes.len() {
            self.owner.status = Status::WriteFailure;
        }
        written
    }
    fn readback_durable(&self) -> bool {
        self.owner.store.readback_durable()
    }
}
