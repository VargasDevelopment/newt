//! Imported-revision authority for verified book sources.
//! MIT; see LICENSE. No allocation, filesystem, or crypto implementation.
use crate::zip_window::source::BookSource;

pub type Digest = [u8; 32];
pub const BLOCK_BYTES: usize = 4096;
pub const HEADER_BYTES: usize = 96;
pub const MAX_BOOK_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_RECEIPT_BYTES: u32 = 96 + (MAX_BOOK_BYTES as u32 / 4096) * 32 + 32;
pub const RETAINED_BLOCKS: usize = 11;
pub const RETAINED_HEADER_BYTES: usize = 64;
const SDK: [u8; 20] = [
    0xcd, 0x6f, 0x5b, 0x83, 0x1c, 0x8f, 0xad, 0x89, 0x97, 0x23, 0xb6, 0xd4, 0x06, 0x58, 0x6f, 0x00,
    0xc5, 0x75, 0xf0, 0x21,
];

/// Trusted local receipt store. Booleans mean complete I/O; adapters loop short I/O.
/// Generation changes on begin/load/abort, not successful promotion. A failed seal
/// may already have published the complete new receipt. Tokens are nonzero leases.
pub trait RevisionStorage {
    // Memory stores have no pathname to defer. Native adapters MUST override
    // both pairs with their actual checked scopes before native acceptance.
    fn begin_immutable_read(&mut self) {}
    fn finish_immutable_read(&mut self) -> bool {
        true
    }
    fn begin_deferred_path_checks(&mut self) {}
    fn finish_deferred_path_checks(&mut self) -> bool {
        true
    }
    fn load(&mut self, bytes: &mut u32) -> bool;
    fn begin(&mut self, bytes: u32) -> bool;
    fn read(&mut self, offset: u32, output: &mut [u8]) -> bool;
    fn write(&mut self, offset: u32, input: &[u8]) -> bool;
    fn seal(&mut self) -> bool;
    fn abort(&mut self);
    fn generation(&self) -> u64;
    fn stable(&mut self, generation: u64) -> bool;
    fn begin_read(&mut self, generation: u64) -> u64 {
        u64::from(self.stable(generation))
    }
    fn stable_read(&mut self, generation: u64, _token: u64) -> bool {
        self.stable(generation)
    }
    fn read_within(&mut self, generation: u64, token: u64, offset: u32, output: &mut [u8]) -> bool {
        self.stable_read(generation, token)
            && self.read(offset, output)
            && self.stable_read(generation, token)
    }
    fn end_read(&mut self, generation: u64, _token: u64) -> bool {
        self.stable(generation)
    }
    fn cancel_read(&mut self, _generation: u64, _token: u64) {}
}

/// Native SHA-256 boundary. Preserve mut-out + bool, including failure timing.
/// `hash` MUST NOT change an in-progress begin/update/finish context (full import
/// hashes a block while the whole-file context is live). No toy hash is suitable.
pub trait RevisionHasher {
    fn begin(&mut self) -> bool;
    fn update(&mut self, input: &[u8]) -> bool;
    fn finish(&mut self, output: &mut Digest) -> bool;
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool;
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Counters {
    pub full_hash_bytes: u64,
    pub verified_bytes: u64,
    pub receipt_hash_bytes: u64,
    pub full_hash_runs: u32,
    pub verified_blocks: u32,
}

/// Source, storage, hashes and scratch remain caller-owned. Two exclusive hasher
/// borrows prevent compact whole/block context aliasing. Source callbacks may
/// revoke their own source/storage, but cannot safely reenter this owner.
pub struct ImportedRevision<'a> {
    core: Core<'a>,
    scratch: &'a mut [u8],
}
struct Core<'a> {
    store: &'a mut dyn RevisionStorage,
    whole: &'a mut dyn RevisionHasher,
    block: Option<&'a mut dyn RevisionHasher>,
    source: Option<&'a mut dyn BookSource>,
    digest: Digest,
    bytes: u64,
    generation: u64,
    epoch: u64,
    blocks: u32,
    receipt_bytes: u32,
    cached_block: u32,
    valid: bool,
    provisional: bool,
    retained_header: bool,
    retain_source_for_reopen: bool,
    counters: Counters,
}
impl<'a> ImportedRevision<'a> {
    pub fn full(
        store: &'a mut dyn RevisionStorage,
        whole: &'a mut dyn RevisionHasher,
        scratch: &'a mut [u8; BLOCK_BYTES],
    ) -> Self {
        Self::new(store, whole, None, scratch)
    }
    /// Compact scratch must be 1..4096 bytes; invalid sizes remain fail-closed.
    pub fn compact(
        store: &'a mut dyn RevisionStorage,
        whole: &'a mut dyn RevisionHasher,
        block: &'a mut dyn RevisionHasher,
        scratch: &'a mut [u8],
    ) -> Self {
        let valid_len = !scratch.is_empty() && scratch.len() < BLOCK_BYTES;
        let mut owner = Self::new(store, whole, Some(block), scratch);
        if !valid_len {
            owner.scratch = &mut [];
        }
        owner
    }
    fn new(
        store: &'a mut dyn RevisionStorage,
        whole: &'a mut dyn RevisionHasher,
        block: Option<&'a mut dyn RevisionHasher>,
        scratch: &'a mut [u8],
    ) -> Self {
        Self {
            core: Core {
                store,
                whole,
                block,
                source: None,
                digest: [0; 32],
                bytes: 0,
                generation: 0,
                epoch: 0,
                blocks: 0,
                receipt_bytes: 0,
                cached_block: u32::MAX,
                valid: false,
                provisional: false,
                retained_header: false,
                retain_source_for_reopen: false,
                counters: Counters::default(),
            },
            scratch,
        }
    }
    /// Scope forwarding uses the one exclusive store already owned by Core.
    pub fn begin_immutable_read(&mut self) {
        self.core.store.begin_immutable_read();
    }
    pub fn finish_immutable_read(&mut self) -> bool {
        self.core.store.finish_immutable_read()
    }
    pub fn begin_deferred_path_checks(&mut self) {
        self.core.store.begin_deferred_path_checks();
    }
    pub fn finish_deferred_path_checks(&mut self) -> bool {
        self.core.store.finish_deferred_path_checks()
    }
    /// Native rejectChangedReceipt closes even a committed descriptor.
    pub fn close_receipt(&mut self) {
        self.core.reset();
        self.core.store.abort();
    }
    pub fn begin_import(&mut self, source: &'a mut dyn BookSource) -> bool {
        self.core.begin_import(source, self.scratch)
    }
    pub fn load(&mut self, source: &'a mut dyn BookSource) -> bool {
        self.core.load(source, self.scratch)
    }
    /// Reopen the stored receipt for the retained source after an interrupted
    /// action closed the receipt handle. Fails closed on any digest or check mismatch.
    pub fn reload_held_receipt(&mut self, expected: Digest) -> bool {
        let Some(source) = self.core.source.take() else {
            return false;
        };
        if self.load(source) && self.core.digest == expected && self.good() {
            true
        } else {
            self.core.reset();
            false
        }
    }
    /// Retain the source handle across a closed receipt for reopen. This confers
    /// no receipt or page authority: reopen must reselect and compare the full digest.
    pub fn retain_source_for_reopen(&mut self, retain: bool) {
        self.core.retain_source_for_reopen = retain;
    }

    /// Session-only publication hook. The shared typed Session will call this
    /// after admission; intentionally unavailable to downstream library callers.
    pub(crate) fn finish_import(&mut self) -> bool {
        self.core.finish_import(self.scratch)
    }
    pub fn reset(&mut self) {
        self.core.reset();
    }
    /// Pinned strictOpen keeps the source/store owner through checked action
    /// exit. This never marks the optional receipt valid.
    pub fn prepare_strict_source(
        &mut self,
        expected: Digest,
        allowed: &dyn Fn() -> bool,
    ) -> crate::zip_container::Status {
        use crate::zip_container::Status;
        let Some(source) = self.core.source.take() else {
            return Status::IoError;
        };
        if !allowed() || !live(source) {
            self.core.source = Some(source);
            return Status::IoError;
        }
        self.core.reset();
        self.core.source = Some(source);
        let mut digest = [0; 32];
        let status = self
            .core
            .hash_complete_source_status(self.scratch, allowed, &mut digest);
        if status != Status::Ok {
            return status;
        }
        if digest != expected {
            Status::Stale
        } else {
            Status::Ok
        }
    }
    /// A short raw-source loan carries no imported-receipt certification.
    pub fn with_held_source<R>(
        &mut self,
        operation: impl FnOnce(&mut dyn BookSource) -> R,
    ) -> Option<R> {
        self.core
            .source
            .as_mut()
            .map(|source| operation(&mut **source))
    }
    pub fn held_source_bytes(&self) -> u64 {
        self.core.source.as_deref().map_or(0, BookSource::size)
    }
    pub fn good(&mut self) -> bool {
        self.core.good()
    }
    pub fn digest(&self) -> &Digest {
        &self.core.digest
    }
    pub fn provisional(&self) -> bool {
        self.core.provisional
    }
    pub fn counters(&self) -> Counters {
        self.core.counters
    }
    pub fn verify_range(&mut self, offset: u64, bytes: u64) -> bool {
        self.core.verify_range(self.scratch, offset, bytes)
    }
    pub fn work_scratch(&mut self) -> &mut [u8] {
        self.core.cached_block = u32::MAX;
        let n = self.scratch.len().min(BLOCK_BYTES);
        &mut self.scratch[..n]
    }
    pub fn digest_scratch(&mut self) -> &mut [u8] {
        self.core.cached_block = u32::MAX;
        self.core.retained_header = false;
        let n = self.scratch.len().min(32768);
        &mut self.scratch[..n]
    }
    /// Scope owns the lending role; Drop withdraws every retained tag. Core holds
    /// no scratch pointer, so withdrawal cannot leave a dangling lender pointer.
    pub fn lend_action_storage<'r>(&'r mut self, scratch: &'r mut [u8]) -> Option<Action<'r, 'a>> {
        if self.core.block.is_none()
            || self.scratch.is_empty()
            || scratch.len() < BLOCK_BYTES
            || scratch.len() > u32::MAX as usize
        {
            return None;
        }
        self.core.retained_header = false;
        Some(Action {
            core: &mut self.core,
            scratch,
        })
    }
    /// Internal parse lease over the 2,048-byte scratch buffer for committed selections.
    pub(crate) fn selection_scratch<'r>(
        &'r mut self,
        scratch: &'r mut [u8; 2048],
    ) -> Option<Action<'r, 'a>> {
        if !self.core.valid
            || self.core.provisional
            || self.core.block.is_none()
            || self.scratch.len() != 256
        {
            return None;
        }
        self.core.retained_header = false;
        Some(Action {
            core: &mut self.core,
            scratch,
        })
    }
}
impl BookSource for ImportedRevision<'_> {
    fn size(&self) -> u64 {
        self.core.bytes
    }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        self.core.read_at(self.scratch, offset, output)
    }
}
include!("producer_receipt.rs");

pub struct Action<'r, 'a> {
    core: &'r mut Core<'a>,
    scratch: &'r mut [u8],
}
impl<'a> Action<'_, 'a> {
    pub fn begin_import(&mut self, source: &'a mut dyn BookSource) -> bool {
        self.core.begin_import(source, self.scratch)
    }
    pub fn load(&mut self, source: &'a mut dyn BookSource) -> bool {
        self.core.load(source, self.scratch)
    }
    pub fn reset(&mut self) {
        self.core.reset();
    }
    pub(crate) fn finish_import(&mut self) -> bool {
        self.core.finish_import(self.scratch)
    }
    pub fn verify_range(&mut self, offset: u64, bytes: u64) -> bool {
        self.core.verify_range(self.scratch, offset, bytes)
    }
    pub fn good(&mut self) -> bool {
        self.core.good()
    }
    pub fn counters(&self) -> Counters {
        self.core.counters
    }
    pub fn digest_scratch(&mut self) -> &mut [u8] {
        self.core.cached_block = u32::MAX;
        self.core.retained_header = false;
        let n = self.scratch.len().min(32768);
        &mut self.scratch[..n]
    }
    pub fn digest(&self) -> &Digest {
        &self.core.digest
    }
    /// Pinned selectRevision's hashSource: the complete current bytes of the
    /// held source, over digest_scratch, through the idle whole context (the
    /// session's hasher_ is the revision's own). False on any read, liveness,
    /// cancellation or hash failure; never a reimport reason.
    pub fn hash_complete_source(
        &mut self,
        allowed: &dyn Fn() -> bool,
        digest: &mut Digest,
    ) -> bool {
        self.core.cached_block = u32::MAX;
        self.core.retained_header = false;
        let n = self.scratch.len().min(32768);
        self.core
            .hash_complete_source(&mut self.scratch[..n], allowed, digest)
    }
    /// Pinned prepareImport inside selectRevision: close the receipt, then
    /// import the source this owner already holds. The source never leaves.
    pub fn reimport(&mut self) -> bool {
        self.core.reimport(self.scratch)
    }
}
impl BookSource for Action<'_, '_> {
    fn size(&self) -> u64 {
        self.core.bytes
    }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        self.core.read_at(self.scratch, offset, output)
    }
}
impl Drop for Action<'_, '_> {
    fn drop(&mut self) {
        self.core.retained_header = false;
    }
}

impl<'a> Core<'a> {
    fn fail(&mut self) -> bool {
        self.valid = false;
        self.cached_block = u32::MAX;
        self.retained_header = false;
        false
    }
    fn reset(&mut self) {
        let abort = self.provisional;
        let generation = self.generation;
        self.epoch = self.epoch.wrapping_add(1);
        self.retained_header = false;
        if !self.retain_source_for_reopen {
            self.source = None;
        }
        self.bytes = 0;
        self.blocks = 0;
        self.receipt_bytes = 0;
        self.valid = false;
        self.provisional = false;
        self.cached_block = u32::MAX;
        self.digest = [0; 32];
        // Detach first; stale provisional owners must not abort a newer receipt.
        if abort && self.store.generation() == generation {
            self.store.abort();
        }
    }
    fn guard(&mut self, token: u64) -> bool {
        let Some(source) = self.source.as_deref_mut() else {
            return false;
        };
        source.size() == self.bytes
            && (if token == 0 {
                self.store.stable(self.generation)
            } else {
                self.store.stable_read(self.generation, token)
            })
            && source.read_at(0, &mut []) == 0
            && (if token == 0 {
                self.store.stable(self.generation)
            } else {
                self.store.stable_read(self.generation, token)
            })
    }
    fn good(&mut self) -> bool {
        self.valid && (self.guard(0) || self.fail())
    }
    fn raw_read(&mut self, mut offset: u64, mut output: &mut [u8], token: u64) -> bool {
        if !self.guard(token) {
            return false;
        }
        while !output.is_empty() {
            let Some(source) = self.source.as_deref_mut() else {
                return false;
            };
            let got = source.read_at(offset, output);
            if got <= 0 || got as usize > output.len() || !self.guard(token) {
                return false;
            }
            offset += got as u64;
            output = &mut output[got as usize..];
        }
        true
    }
    fn block_length(&self, block: u32) -> usize {
        (self.bytes - u64::from(block) * BLOCK_BYTES as u64).min(BLOCK_BYTES as u64) as usize
    }
    fn begin_import(&mut self, source: &'a mut dyn BookSource, scratch: &mut [u8]) -> bool {
        self.reset();
        self.bytes = source.size();
        self.source = Some(source);
        if scratch.is_empty() || self.bytes == 0 || self.bytes > MAX_BOOK_BYTES {
            return self.fail();
        }
        self.blocks = self.bytes.div_ceil(BLOCK_BYTES as u64) as u32;
        self.receipt_bytes = HEADER_BYTES as u32 + self.blocks * 32 + 32;
        if !self.store.begin(self.receipt_bytes) {
            return self.fail();
        }
        self.generation = self.store.generation();
        self.provisional = true;
        if !self.whole.begin() {
            return self.fail();
        }
        self.counters.full_hash_runs = self.counters.full_hash_runs.wrapping_add(1);
        if !self.import_blocks(scratch) || !self.whole.finish(&mut self.digest) {
            return self.fail();
        }
        let mut header = [0u8; HEADER_BYTES];
        header[..8].copy_from_slice(b"NTRV0001");
        header[8..12].copy_from_slice(&1u32.to_le_bytes());
        header[12..16].copy_from_slice(&(BLOCK_BYTES as u32).to_le_bytes());
        header[16..24].copy_from_slice(&self.bytes.to_le_bytes());
        header[24..28].copy_from_slice(&self.blocks.to_le_bytes());
        header[32..64].copy_from_slice(&self.digest);
        header[64..84].copy_from_slice(&SDK);
        if !self.store.write(0, &header) || !self.guard(0) {
            return self.fail();
        }
        self.valid = true;
        true
    }
    fn import_blocks(&mut self, scratch: &mut [u8]) -> bool {
        let batch = (scratch.len() / BLOCK_BYTES).min(8);
        let mut block = 0;
        while block < self.blocks {
            if batch < 2 {
                let count = self.block_length(block);
                let mut hash = [0; 32];
                if !self.import_block(scratch, block, count, &mut hash)
                    || !self.store.write(HEADER_BYTES as u32 + block * 32, &hash)
                {
                    return false;
                }
                self.counters.full_hash_bytes =
                    self.counters.full_hash_bytes.wrapping_add(count as u64);
                block += 1;
            } else {
                let take = batch.min((self.blocks - block) as usize);
                let bytes = (self.bytes - u64::from(block) * BLOCK_BYTES as u64)
                    .min((take * BLOCK_BYTES) as u64) as usize;
                if !self.raw_read(
                    u64::from(block) * BLOCK_BYTES as u64,
                    &mut scratch[..bytes],
                    0,
                ) {
                    return false;
                }
                for index in 0..take {
                    let count = self.block_length(block);
                    let mut hash = [0; 32];
                    let input = &scratch[index * BLOCK_BYTES..index * BLOCK_BYTES + count];
                    if !self.whole.update(input) {
                        return false;
                    }
                    let hashed = if let Some(h) = self.block.as_deref_mut() {
                        h.begin() && h.update(input) && h.finish(&mut hash)
                    } else {
                        self.whole.hash(input, &mut hash)
                    };
                    if !hashed
                        || !self.guard(0)
                        || !self.store.write(HEADER_BYTES as u32 + block * 32, &hash)
                    {
                        return false;
                    }
                    self.counters.full_hash_bytes =
                        self.counters.full_hash_bytes.wrapping_add(count as u64);
                    block += 1;
                }
            }
        }
        true
    }
    fn import_block(
        &mut self,
        scratch: &mut [u8],
        block: u32,
        count: usize,
        hash: &mut Digest,
    ) -> bool {
        if self.block.is_none() {
            return self.raw_read(
                u64::from(block) * BLOCK_BYTES as u64,
                &mut scratch[..count],
                0,
            ) && self.whole.update(&scratch[..count])
                && self.whole.hash(&scratch[..count], hash);
        }
        if !self.block.as_deref_mut().is_some_and(|h| h.begin()) {
            return false;
        }
        let mut at = 0;
        while at < count {
            let take = scratch.len().min(count - at);
            if !self.raw_read(
                u64::from(block) * BLOCK_BYTES as u64 + at as u64,
                &mut scratch[..take],
                0,
            ) || !self.whole.update(&scratch[..take])
                || !self
                    .block
                    .as_deref_mut()
                    .is_some_and(|h| h.update(&scratch[..take]))
            {
                return false;
            }
            at += take;
        }
        self.block.as_deref_mut().is_some_and(|h| h.finish(hash))
    }
    #[inline(never)]
    fn load(&mut self, source: &'a mut dyn BookSource, scratch: &mut [u8]) -> bool {
        self.reset();
        self.source = Some(source);
        if scratch.is_empty() || !self.store.load(&mut self.receipt_bytes) {
            return self.fail();
        }
        self.generation = self.store.generation();
        let mut header = [0; HEADER_BYTES];
        if self.receipt_bytes < HEADER_BYTES as u32 + 64
            || self.receipt_bytes > MAX_RECEIPT_BYTES
            || !self.store.read(0, &mut header)
            || &header[..8] != b"NTRV0001"
            || get32(&header[8..12]) != 1
            || get32(&header[12..16]) != BLOCK_BYTES as u32
            || get32(&header[28..32]) != 0
            || header[64..84] != SDK
            || header[84..].iter().any(|&b| b != 0)
        {
            return self.fail();
        }
        self.bytes = get64(&header[16..24]);
        self.blocks = get32(&header[24..28]);
        if self.bytes == 0
            || self.bytes > MAX_BOOK_BYTES
            || self.source.as_deref().map(BookSource::size) != Some(self.bytes)
            || u64::from(self.blocks) != self.bytes.div_ceil(BLOCK_BYTES as u64)
            || self.receipt_bytes != HEADER_BYTES as u32 + self.blocks * 32 + 32
        {
            return self.fail();
        }
        self.digest.copy_from_slice(&header[32..64]);
        let mut actual = [0; 32];
        if !self.receipt_hash(scratch, &mut actual, true)
            || !self.digest_matches(self.receipt_bytes - 32, &actual)
            || !self.guard(0)
        {
            return self.fail();
        }
        self.cached_block = u32::MAX;
        self.valid = true;
        true
    }
    /// Pinned hashCompleteSource (reader_revision_verification.h:134-149).
    fn hash_complete_source(
        &mut self,
        buffer: &mut [u8],
        allowed: &dyn Fn() -> bool,
        digest: &mut Digest,
    ) -> bool {
        self.hash_complete_source_status(buffer, allowed, digest)
            == crate::zip_container::Status::Ok
    }
    fn hash_complete_source_status(
        &mut self,
        buffer: &mut [u8],
        allowed: &dyn Fn() -> bool,
        digest: &mut Digest,
    ) -> crate::zip_container::Status {
        use crate::zip_container::Status;
        let Some(source) = self.source.as_deref_mut() else {
            return Status::IoError;
        };
        let bytes = source.size();
        if buffer.is_empty() || bytes == 0 || bytes > MAX_BOOK_BYTES || !self.whole.begin() {
            return Status::IoError;
        }
        let mut at = 0;
        while at < bytes {
            let want = (bytes - at).min(buffer.len() as u64) as usize;
            let got = source.read_at(at, &mut buffer[..want]);
            if !allowed()
                || got <= 0
                || got as usize > want
                || !live(source)
                || !self.whole.update(&buffer[..got as usize])
            {
                return Status::IoError;
            }
            at += got as u64;
        }
        if self.whole.finish(digest) && allowed() && live(source) {
            Status::Ok
        } else {
            Status::Stale
        }
    }
    /// Pinned prepareImport: close() resets the revision and the receipt.
    fn reimport(&mut self, scratch: &mut [u8]) -> bool {
        let Some(source) = self.source.take() else {
            return self.fail();
        };
        self.reset();
        self.store.abort();
        self.begin_import(source, scratch)
    }
    fn digest_matches(&mut self, offset: u32, actual: &Digest) -> bool {
        let mut expected = [0; 32];
        self.store.read(offset, &mut expected) && expected == *actual
    }
    fn finish_import(&mut self, scratch: &mut [u8]) -> bool {
        if !self.valid || !self.provisional || !self.guard(0) {
            return self.fail();
        }
        let mut hash = [0; 32];
        if !self.receipt_hash(scratch, &mut hash, false)
            || !self.store.write(self.receipt_bytes - 32, &hash)
            || !self.guard(0)
            || !self.store.seal()
        {
            return self.fail();
        }
        self.provisional = false;
        self.guard(0) || self.fail()
    }
    fn receipt_hash(&mut self, scratch: &mut [u8], output: &mut Digest, loaded: bool) -> bool {
        if !loaded {
            if !self.whole.begin() {
                return false;
            }
            let mut offset = 0;
            while offset < self.receipt_bytes - 32 {
                let count = scratch
                    .len()
                    .min(BLOCK_BYTES)
                    .min((self.receipt_bytes - 32 - offset) as usize);
                if !self.store.read(offset, &mut scratch[..count])
                    || !self.whole.update(&scratch[..count])
                {
                    return false;
                }
                offset += count as u32;
                self.counters.receipt_hash_bytes =
                    self.counters.receipt_hash_bytes.wrapping_add(count as u64);
            }
            self.cached_block = u32::MAX;
            return self.whole.finish(output);
        }
        // Load leases deliberately do not depend on valid_: header/footer
        // validation publishes that flag only after this checked checksum pass.
        let mut lease = ReadLease::new(self.store, self.generation);
        if !lease.current() || !self.whole.begin() || !lease.current() {
            return false;
        }
        let mut offset = 0;
        while offset < self.receipt_bytes - 32 {
            let count = scratch
                .len()
                .min(BLOCK_BYTES)
                .min((self.receipt_bytes - 32 - offset) as usize);
            if !lease.current()
                || !lease.read(offset, &mut scratch[..count])
                || !lease.current()
                || !self.whole.update(&scratch[..count])
                || !lease.current()
            {
                return false;
            }
            offset += count as u32;
            self.counters.receipt_hash_bytes =
                self.counters.receipt_hash_bytes.wrapping_add(count as u64);
        }
        if !self.whole.finish(output) || !lease.current() || !lease.current() || !lease.finish() {
            return false;
        }
        self.cached_block = u32::MAX;
        true
    }
    fn read_at(&mut self, scratch: &mut [u8], mut offset: u64, output: &mut [u8]) -> i32 {
        if !self.valid {
            return -1;
        }
        if !self.guard(0) {
            self.fail();
            return -1;
        }
        if output.is_empty() || offset >= self.bytes {
            return 0;
        }
        let count = output
            .len()
            .min(BLOCK_BYTES)
            .min((self.bytes - offset) as usize);
        let mut at = 0;
        while at < count {
            let block = (offset / BLOCK_BYTES as u64) as u32;
            let begin = (offset % BLOCK_BYTES as u64) as usize;
            let take = (count - at).min(self.block_length(block) - begin);
            if !self.verify_copy(
                scratch,
                block,
                Some(&mut output[at..at + take]),
                begin,
                take,
            ) {
                return -1;
            }
            offset += take as u64;
            at += take;
        }
        if !self.guard(0) {
            self.fail();
            return -1;
        }
        count as i32
    }
    fn verify_range(&mut self, scratch: &mut [u8], mut offset: u64, mut bytes: u64) -> bool {
        if !self.valid {
            return false;
        }
        if !self.guard(0) || offset > self.bytes || bytes > self.bytes - offset {
            return self.fail();
        }
        while bytes != 0 {
            let block = (offset / BLOCK_BYTES as u64) as u32;
            if !self.verify_copy(scratch, block, None, 0, 0) {
                return false;
            }
            let take = bytes.min(self.block_length(block) as u64 - offset % BLOCK_BYTES as u64);
            offset += take;
            bytes -= take;
        }
        self.guard(0) || self.fail()
    }
    fn verify_copy(
        &mut self,
        scratch: &mut [u8],
        block: u32,
        output: Option<&mut [u8]>,
        begin: usize,
        bytes: usize,
    ) -> bool {
        if self.block.is_none() {
            if !self.guard(0) || !self.valid {
                return self.fail();
            }
            if self.cached_block != block {
                let count = self.block_length(block);
                let mut actual = [0; 32];
                if !self.raw_read(
                    u64::from(block) * BLOCK_BYTES as u64,
                    &mut scratch[..count],
                    0,
                ) || !self.whole.hash(&scratch[..count], &mut actual)
                    || !self.digest_matches(HEADER_BYTES as u32 + block * 32, &actual)
                    || !self.guard(0)
                {
                    return self.fail();
                }
                self.cached_block = block;
                self.count_verified(count);
            }
            if let Some(dst) = output {
                dst.copy_from_slice(&scratch[begin..begin + bytes]);
            }
            return true;
        }
        let slots = scratch
            .len()
            .saturating_sub(BLOCK_BYTES + RETAINED_HEADER_BYTES)
            / BLOCK_BYTES;
        let slots = slots.min(RETAINED_BLOCKS);
        if slots == 0 {
            return self.authenticate_copy(scratch, block, output, begin, bytes);
        }
        if !self.retained_header {
            scratch[BLOCK_BYTES..BLOCK_BYTES + RETAINED_HEADER_BYTES].fill(0);
            self.retained_header = true;
        }
        let mut held = None;
        for index in 0..slots {
            if retained_word(scratch, index + 1) == block + 1 {
                held = Some(index);
                break;
            }
        }
        let index = if let Some(index) = held {
            index
        } else {
            let index = retained_word(scratch, 0) as usize % slots;
            set_retained_word(scratch, 0, ((index + 1) % slots) as u32);
            set_retained_word(scratch, index + 1, 0);
            // Split disjoint working and retained storage: no raw pointer alias.
            let (work, rest) = scratch.split_at_mut(BLOCK_BYTES);
            let start = RETAINED_HEADER_BYTES + index * BLOCK_BYTES;
            let count = self.block_length(block);
            if !self.authenticate_copy(work, block, Some(&mut rest[start..start + count]), 0, count)
            {
                return false;
            }
            // Publish the tag only after the complete authenticated copy.
            set_retained_word(scratch, index + 1, block + 1);
            index
        };
        if let Some(dst) = output {
            let start = BLOCK_BYTES + RETAINED_HEADER_BYTES + index * BLOCK_BYTES + begin;
            dst.copy_from_slice(&scratch[start..start + bytes]);
        }
        true
    }
    fn authenticate_copy(
        &mut self,
        scratch: &mut [u8],
        block: u32,
        mut output: Option<&mut [u8]>,
        begin: usize,
        bytes: usize,
    ) -> bool {
        let count = self.block_length(block);
        // Split all callback-bearing dependencies from state before lending the
        // store to the RAII lease. No callback receives an owner borrow.
        let generation = self.generation;
        let mut receipt = ReadLease::new(self.store, generation);
        let Some(source) = self.source.as_deref_mut() else {
            drop(receipt);
            return self.fail();
        };
        let Some(hasher) = self.block.as_deref_mut() else {
            drop(receipt);
            return self.fail();
        };
        let result = (|| {
            if !lease_guard(source, self.bytes, &mut receipt) || !self.valid || !hasher.begin() {
                return false;
            }
            let mut at = 0;
            while at < count {
                let take = scratch.len().min(count - at);
                if !lease_raw_read(
                    source,
                    self.bytes,
                    &mut receipt,
                    u64::from(block) * BLOCK_BYTES as u64 + at as u64,
                    &mut scratch[..take],
                ) || !hasher.update(&scratch[..take])
                {
                    return false;
                }
                let first = at.max(begin);
                let last = (at + take).min(begin + bytes);
                if let Some(dst) = output.as_deref_mut() {
                    if last > first {
                        dst[first - begin..last - begin]
                            .copy_from_slice(&scratch[first - at..last - at]);
                    }
                }
                at += take;
            }
            let mut actual = [0; 32];
            if !hasher.finish(&mut actual) {
                return false;
            }
            let mut at = 0;
            while at < actual.len() {
                let take = scratch.len().min(actual.len() - at);
                if !receipt.read(
                    HEADER_BYTES as u32 + block * 32 + at as u32,
                    &mut scratch[..take],
                ) || scratch[..take] != actual[at..at + take]
                {
                    return false;
                }
                at += take;
            }
            lease_guard(source, self.bytes, &mut receipt) && receipt.finish()
        })();
        // Failure invalidates before cancel (the native fail unwinds its lease).
        if !result {
            self.valid = false;
            self.cached_block = u32::MAX;
            self.retained_header = false;
        }
        drop(receipt);
        if result {
            self.count_verified(count);
        }
        result
    }
    fn count_verified(&mut self, bytes: usize) {
        self.counters.verified_bytes = self.counters.verified_bytes.wrapping_add(bytes as u64);
        self.counters.verified_blocks = self.counters.verified_blocks.wrapping_add(1);
    }
}

/// A nonzero token is cancelled on every unfinished path, including unwinding.
struct ReadLease<'s> {
    store: &'s mut dyn RevisionStorage,
    generation: u64,
    token: u64,
}
impl<'s> ReadLease<'s> {
    fn new(store: &'s mut dyn RevisionStorage, generation: u64) -> Self {
        let token = store.begin_read(generation);
        Self {
            store,
            generation,
            token,
        }
    }
    fn current(&mut self) -> bool {
        self.token != 0 && self.store.stable_read(self.generation, self.token)
    }
    fn read(&mut self, offset: u32, output: &mut [u8]) -> bool {
        self.token != 0
            && self
                .store
                .read_within(self.generation, self.token, offset, output)
    }
    fn finish(&mut self) -> bool {
        if self.token == 0 {
            return false;
        }
        let result = self.store.end_read(self.generation, self.token);
        self.token = 0;
        result
    }
}
impl Drop for ReadLease<'_> {
    fn drop(&mut self) {
        if self.token != 0 {
            self.store.cancel_read(self.generation, self.token);
        }
    }
}
fn lease_guard(source: &mut dyn BookSource, bytes: u64, receipt: &mut ReadLease<'_>) -> bool {
    source.size() == bytes
        && receipt.current()
        && source.read_at(0, &mut []) == 0
        && receipt.current()
}
fn lease_raw_read(
    source: &mut dyn BookSource,
    bytes: u64,
    receipt: &mut ReadLease<'_>,
    mut at: u64,
    mut output: &mut [u8],
) -> bool {
    if !lease_guard(source, bytes, receipt) {
        return false;
    }
    while !output.is_empty() {
        let got = source.read_at(at, output);
        if got <= 0 || got as usize > output.len() || !lease_guard(source, bytes, receipt) {
            return false;
        }
        at += got as u64;
        output = &mut output[got as usize..];
    }
    true
}
fn get32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
/// Pinned ReaderRevisionSession::live: a nonempty source answers a probe.
fn live(source: &mut dyn BookSource) -> bool {
    source.size() != 0 && source.read_at(0, &mut []) == 0
}
fn get64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}
fn retained_word(bytes: &[u8], index: usize) -> u32 {
    let at = BLOCK_BYTES + 4 * index;
    get32(&bytes[at..at + 4])
}
fn set_retained_word(bytes: &mut [u8], index: usize, value: u32) {
    let at = BLOCK_BYTES + 4 * index;
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[path = "revision_state.rs"]
mod state;
pub use state::{RevisionPorts, RevisionState, StateLoanError};

#[path = "imported_revision/action_storage.rs"]
mod action_storage;
pub use action_storage::OriginalScratch;

// Session is a child of its authority owner.
#[path = "session.rs"]
pub mod session;

#[path = "imported_revision/select.rs"]
mod select;
