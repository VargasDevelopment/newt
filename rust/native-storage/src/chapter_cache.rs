//! Native chapter cache storage owner (`Pages/` and `Meta/`). MIT.
//! Child of port: shares the actual native owner, not a second filesystem loan.
//! Pages/ holds block-envelope pages; every other record is footered in Meta/.
use super::{Authority, FilePort, MissingProbe, Native, Operation, ProducerPolicy, Route, FilePortState, LoanError, CloseOutcome};
use newt_sdk_heap::Bytes;
use newt_core::cache_names::namespace::{PAGE_CREDIT_UNIT, PAGE_ENTRY_GRANT, cache_folder, envelope_kind, hex};
use newt_core::page_cache::CacheStorage;
use newt_core::page_envelope::{self as envelope, BLOCK_BYTES, MANIFEST_BYTES, MAX_EXTENT, Query};

pub const MAX_BYTES: u32 = 4 * 1024 * 1024;
pub const FOOTER_BYTES: u32 = 12;
/// Full-CRC metadata opening reads admit at most this payload; Pages use blocks.
pub const CACHE_READ_BYTES: u32 = 128 * 1024;
/// The original's default attempt budget for one checked cache read.
pub const CACHE_READ_ATTEMPTS: u32 = 128;
const _: () = assert!(MAX_BYTES == envelope::MAX_PAYLOAD);

#[path = "chapter_cache_state.rs"]
mod state;
pub use state::ChapterCacheState;
use state::{State, Lookup};

/// Borrowed Pages/Meta view. Dropping it owes no I/O or name cleanup.
pub struct ChapterCacheStorage<'a, S: Native> {
    file: FilePort<'a, S>,
    state: &'a mut State,
}
impl<'a, S: Native> ChapterCacheStorage<'a, S> {
    pub fn lend(
        owner: &'a mut ChapterCacheState, native: &'a mut S, route: &'a Route<'a>,
    ) -> core::result::Result<Self, LoanError> {
        Ok(Self { file: FilePort::lend(&mut owner.file, native, route, "Pages")?, state: &mut owner.state })
    }
    /// A failed source verification revokes this digest's derived index too.
    #[inline(never)]
    pub fn invalidate_index(&mut self, digest: &[u8;32]) -> bool {
        newt_core::page_cache::ReadScope::invalidate_index(&mut self.access(), digest)
    }
    /// A foreground loan, without producer policy. Dropping it owes nothing.
    pub fn access(&mut self) -> ChapterCacheAccess<'_, 'a, S> {
        ChapterCacheAccess {
            file: self.file.operation(),
            state: &mut *self.state,
        }
    }
    /// beginProducer, one synchronous attempt, then endProducer. The work
    /// cannot skip the cleanup, which runs while the policy is still attached
    /// so cleanup failures and observed cancellation stay attributed to it.
    /// None: the previous lease did not release cleanly, so nothing ran.
    pub fn produce<R>(
        &mut self,
        policy: &ProducerPolicy<'_>,
        work: impl FnOnce(&mut ChapterCacheAccess<'_, 'a, S>) -> R,
    ) -> Option<R> {
        let mut idle = self.access();
        let lease = idle.reset();
        idle.forget_absent();
        if !idle.file.released_cleanly(lease) {
            return None;
        }
        let mut attempt = ProducerAttempt(ChapterCacheAccess {
            file: self.file.attach_producer(policy)?,
            state: &mut *self.state,
        });
        Some(work(&mut attempt.0))
    }
}

/// One loan of the owner: the SDK cache storage, its read authority and the
/// query source. A producer attempt's loan carries the attempt policy.
pub struct ChapterCacheAccess<'op, 'a, S: Native> {
    file: Operation<'op, 'a, S>,
    state: &'op mut State,
}

// The callback cannot take or forget this owner. Cleanup keeps the policy
// attached through descriptor release, including a caught host unwind.
struct ProducerAttempt<'op, 'a, S: Native>(ChapterCacheAccess<'op, 'a, S>);

impl<S: Native> Drop for ProducerAttempt<'_, '_, S> {
    fn drop(&mut self) {
        self.0.close();
        self.0.file.detach_producer();
        self.0.forget_absent();
    }
}

impl<S: Native> ChapterCacheAccess<'_, '_, S> {
    /// Same transient allocation at the frozen validation/finalization sites.
    fn scratch() -> Option<Bytes> {
        Bytes::try_new(BLOCK_BYTES as usize)
    }

    /// Presence hint only. A retained page descriptor answers from its
    /// existing lease; every query still verifies at its own checked exit.
    pub fn exists(&mut self, name: &[u8]) -> bool {
        let lease = self.file.generation();
        if self.state.read_active
            && valid_name(name).is_some()
            && self.file.envelope_read()
            && self.same(name)
        {
            return self.file.allowed() && self.file.released_cleanly(lease);
        }
        self.file_size(name) >= 0
    }
    /// Read-only admission probe: never withdraws a displayed Pages lease.
    /// `resolved` false means present but not yet CRC-verified.
    pub fn probe_page(
        &mut self,
        name: &[u8],
        cancel: &dyn Authority,
        resolved: &mut bool,
    ) -> MissingProbe {
        *resolved = true;
        if cancel.cancelled() {
            return MissingProbe::Stopped;
        }
        let Some(text) = valid_name(name) else {
            return MissingProbe::Denied;
        };
        if self.state.read_active && self.same(name) {
            self.file.begin_immutable_read();
            let found = self.file_size(name) >= 0
                && (!self.file.envelope_read() || self.page_hint(true, Some(cancel)));
            let fresh = self.file.finish_immutable_read();
            if cancel.cancelled() {
                return MissingProbe::Stopped;
            }
            if !fresh {
                return MissingProbe::Denied;
            }
            return if found {
                MissingProbe::Complete
            } else if self.state.lookup == Lookup::Invalid {
                MissingProbe::Ready
            } else {
                MissingProbe::Denied
            };
        }
        let hint = self.file.probe_missing(text, cancel);
        if hint == MissingProbe::Complete {
            *resolved = false;
        }
        hint
    }
    pub fn pinned_page(&self) -> Option<&str> {
        if self.state.read_active {
            self.file.pinned_page()
        } else {
            None
        }
    }
    /// Durable identity publication needs a confirmed miss, not the folded
    /// shortcut or an indistinguishable I/O error.
    pub fn identity_file_size(&mut self, name: &[u8], uncertain: &mut bool) -> i64 {
        self.forget_absent_name(name);
        let size = self.file_size(name);
        *uncertain = size < 0 && self.state.lookup == Lookup::Unknown;
        size
    }
    /// One immutable scope covers the record's CRC, footer, payload and the
    /// final authority check; nothing is published before that exit.
    pub fn inspect_identity(&mut self, name: &[u8], out: &mut [u8], uncertain: &mut bool) -> bool {
        if !self.begin_cache_read(false, CACHE_READ_ATTEMPTS) {
            *uncertain = true;
            return false;
        }
        let size = self.identity_file_size(name, uncertain);
        let exact = size == out.len() as i64;
        let read = exact && self.read_at(name, 0, out) == out.len() as i32;
        let fresh = self.finish_cache_read();
        if (exact && !read) || !fresh {
            *uncertain = true;
        }
        read && fresh
    }
    /// CacheReadAuthority::begin. A query is bound to the retained descriptor
    /// and generation; its bytes stay untrusted until the query verifies them.
    pub fn begin_page_read(&mut self, opening: bool, query: Option<&mut Query<'_>>) -> bool {
        if !self.begin_cache_read(opening, CACHE_READ_ATTEMPTS) {
            return false;
        }
        let Some(query) = query else {
            return true; // a presence probe grants no consumption capability
        };
        let size = if query.active {
            self.page_query_size(query.name.unwrap_or_default())
        } else {
            -1
        };
        if size < 0 || !self.file.envelope_read() {
            query.finish();
            self.finish_cache_read();
            return false;
        }
        query.generation = self.file.generation();
        query.extent = self.file.bytes();
        query.bound = true;
        true
    }
    pub fn begin_cache_read(&mut self, opening: bool, attempts: u32) -> bool {
        let limit = if opening { CACHE_READ_BYTES } else { MAX_BYTES };
        if !self.file.port.route.begin_cache_read(limit, attempts) {
            return false;
        }
        self.file.begin_immutable_read();
        true
    }
    /// CacheReadAuthority::finish: the checked exit of one read scope.
    pub fn finish_cache_read(&mut self) -> bool {
        let fresh = self.file.finish_immutable_read();
        self.file.port.route.finish_cache_read() && fresh
    }
    pub fn close(&mut self) {
        self.adopt_legacy();
        self.reset();
    }
    pub fn release_for_admission(&mut self) -> bool {
        let lease = self.reset();
        self.forget_absent();
        self.file.released_cleanly(lease)
    }
    /// A producer pause owns its suspended provisional writer. Durable state
    /// borrows the freed descriptor without resetting that transaction.
    /// False when the close failed; the next release would forget that.
    pub fn release_for_state(&mut self) -> bool {
        if self.file.producing() {
            return true;
        }
        self.close();
        self.file.released_cleanly(self.file.generation())
    }
    /// Keeps exactly a validated page descriptor; metadata opens displace it.
    pub fn close_metadata(&mut self) {
        if !self.state.read_active || !self.file.envelope_read() {
            self.close();
        }
    }
    pub fn set_producer_allowance(&mut self, units: u8, entries: u8) {
        self.file
            .set_write_credit(u64::from(units.min(127)) * u64::from(PAGE_CREDIT_UNIT));
        self.state.entries = entries.min(PAGE_ENTRY_GRANT);
    }
    pub fn producing(&self) -> bool {
        self.file.producing()
    }
    /// A producer pause may find the validated read descriptor that `exists()`
    /// keeps after a segment check (between segments, nothing is writing). The
    /// port accepts only idle or a live writer, and State needs the descriptor,
    /// so drop that read first; the next lookup reopens and revalidates it.
    pub fn suspend_producer_write(&mut self) -> bool {
        if self.state.read_active && !self.file.writing() {
            self.close();
        }
        self.file.suspend_producer_write()
    }
    pub fn resume_producer_write(&mut self) -> bool {
        self.file.resume_producer_write()
    }
    pub fn producer_allowance(&self) -> u64 {
        self.file.write_credit()
    }
    /// Session boundary: later lookups prove absence afresh.
    pub fn forget_absent(&mut self) {
        self.state.absent = [0; 3];
    }
    pub fn begin_deferred_path_checks(&mut self) {
        self.file.begin_deferred_path_checks()
    }
    pub fn finish_deferred_path_checks(&mut self) -> bool {
        self.file.finish_deferred_path_checks()
    }

    fn same(&self, name: &[u8]) -> bool {
        core::str::from_utf8(c_str(name)).is_ok_and(|name|self.file.same_name(name))
    }
    /// FilePort::pageName, widened: the final suffix when it names a
    /// block-envelope file (a page or a book index).
    fn envelope_name(&self) -> Option<&[u8]> {
        let suffix = self.file.suffix().as_bytes();
        envelope_kind(suffix).is_some().then_some(suffix)
    }
    fn envelope_route(&mut self,out:&mut envelope::Route,canonical:bool)->bool {
        if self.envelope_name().and_then(newt_core::book_index::FileName::parse).is_some() {
            // The short filename cannot supply identity. Use the full digest
            // in the payload header; checked query callers independently bind
            // their expected digest before trusting any index records.
            self.file.read(newt_core::book_index::DIGEST_AT,out)
        } else {envelope::route(self.envelope_name(),out,canonical)}
    }
    fn page_query_size(&mut self, name: &[u8]) -> i64 {
        if self.state.read_active && self.same(name) {
            let lease = self.file.generation();
            if self.file.defer_page_path_check() {
                return i64::from(self.state.payload);
            }
            // Match file_size's failed-generation latch: release only this descriptor.
            if self.file.generation() == lease {
                self.file.release();
            }
            return -1;
        }
        self.file_size(name)
    }
    /// Producers spend the grant per byte before each write (an abort keeps
    /// the debit); every other writer only debits.
    fn spend(&mut self, bytes: u64) -> bool {
        if self.file.producing() && bytes > self.file.write_credit() {
            return false;
        }
        self.file.charge_write(bytes);
        true
    }
    fn reset(&mut self) -> u64 {
        self.state.payload = 0;
        self.state.crc = 0;
        self.state.read_active = false;
        self.state.legacy = false;
        self.file.release()
    }
    fn validate(&mut self) -> bool {
        // Only metadata's full CRC scan is byte-capped at opening. Pages check
        // their directory and consumed blocks under the same time/attempt scope.
        let bytes = self.file.bytes();
        if !self.file.port.route.cache_read_live()
            || (self.envelope_name().is_none()
                && !self.file.port.route.cache_read_size(bytes.saturating_sub(FOOTER_BYTES))) {
            return self.file.settle(false);
        }
        // A private CRC may borrow an enclosing checked scope; consumers
        // cannot publish it before their own checked exit.
        if self.file.checks_deferred() {
            return self.validate_payload();
        }
        self.file.begin_immutable_read();
        let valid = self.validate_payload();
        if !valid && (self.file.writing() || self.state.lookup == Lookup::Unknown) {
            self.file.settle(false);
        }
        if !self.file.finish_immutable_read() {
            self.state.lookup = Lookup::Unknown;
            return self.file.settle(false);
        }
        valid
    }
    fn validate_payload(&mut self) -> bool {
        self.state.lookup = Lookup::Invalid;
        if self.envelope_name().is_some() {
            return self.page_hint(false, None);
        }
        if self.file.bytes() < FOOTER_BYTES {
            return false;
        }
        let bytes = self.file.bytes() - FOOTER_BYTES;
        let mut footer = [0; FOOTER_BYTES as usize];
        if !self.file.read(bytes, &mut footer) {
            self.state.lookup = Lookup::Unknown;
            return false;
        }
        if &footer[8..] != b"NTCC" || get32(&footer[4..]) != bytes {
            return false;
        }
        // The full CRC stays mandatory. A transient batch keeps the stack flat
        // and retains no index; refusal rejects optional data unpublished.
        let Some(mut scratch) = Self::scratch() else {
            self.state.lookup = Lookup::Unknown;
            return false;
        };
        let mut crc = u32::MAX;
        let mut at = 0;
        while at < bytes {
            if !self.file.port.route.cache_read_live() {
                self.state.lookup = Lookup::Unknown;
                return false;
            }
            let take = (bytes - at).min(BLOCK_BYTES);
            let batch = &mut scratch.as_mut()[..take as usize];
            if !self.file.read(at, batch) {
                self.state.lookup = Lookup::Unknown;
                return false;
            }
            if self.file.producing() && !self.file.allowed() {
                return false;
            }
            crc = envelope::crc(crc, batch);
            at += take;
            if self.file.producing() && at < bytes && !self.file.allowed() {
                return false;
            }
        }
        // Settle the final checksum before observing any later cancellation.
        if crc ^ u32::MAX != get32(&footer) {
            if self.file.writing() {
                self.file.settle(false);
            }
            return false;
        }
        if self.file.producing() && !self.file.allowed() {
            return false;
        }
        self.state.payload = bytes;
        true
    }
    /// Foreground: a presence and extent hint only. Every producer admission,
    /// including a retained lease, proves the complete candidate first.
    fn page_hint(&mut self, complete: bool, cancel: Option<&dyn Authority>) -> bool {
        self.state.lookup = Lookup::Invalid;
        let mut route = [0; 32];
        let extent = self.file.bytes();
        if extent < MANIFEST_BYTES || !self.envelope_route(&mut route,true) {
            return false;
        }
        let mut manifest = [0; MANIFEST_BYTES as usize];
        if !self.file.read(extent - MANIFEST_BYTES, &mut manifest) {
            self.state.lookup = Lookup::Unknown;
            return false;
        }
        let mut payload = 0;
        if !envelope::inspect_for(self.envelope_name(), &manifest, extent, &route, &mut payload) {
            return false;
        }
        if complete || self.file.producing() {
            let Some(mut scratch) = Self::scratch() else {
                self.state.lookup = Lookup::Unknown;
                return false;
            };
            if !self.verify_page_payload(&route, payload, scratch.as_mut(), cancel) {
                return false;
            }
        }
        self.state.payload = payload;
        true
    }
    /// Directory entries come from SD readback, never from input writes; the
    /// finished candidate then gets a second full readback before promotion.
    fn finalize_page(&mut self) -> bool {
        let payload = self.state.payload;
        if payload == 0 || payload > MAX_BYTES {
            return false;
        }
        let mut route = [0; 32];
        let Some(mut scratch) = Self::scratch() else {
            return false;
        };
        if !self.envelope_route(&mut route,false) {
            return false;
        }
        let (mut whole, mut directory) = (u32::MAX, u32::MAX);
        let count = envelope::blocks(payload);
        if !self.spend(u64::from(4 * count + MANIFEST_BYTES)) {
            return false;
        }
        let mut at = 0;
        for block in 0..count {
            let take = BLOCK_BYTES.min(payload - at);
            let data = &mut scratch.as_mut()[..take as usize];
            #[cfg(feature = "page-turn-trace")]
            let read_started = self.file.trace_micros();
            let read_ok = self.file.read(at, data);
            #[cfg(feature = "page-turn-trace")]
            {
                self.state.trace.seal.read_us = self
                    .state
                    .trace
                    .seal
                    .read_us
                    .wrapping_add(self.file.trace_micros().wrapping_sub(read_started));
            }
            if !read_ok || !self.file.allowed() {
                return false;
            }
            #[cfg(feature = "page-turn-trace")]
            let crc_started = self.file.trace_micros();
            let Some(checked) = self.readback_crc(&route, block, data, &mut whole, None) else {
                return false;
            };
            let entry = checked.to_le_bytes();
            directory = envelope::crc(directory, &entry);
            #[cfg(feature = "page-turn-trace")]
            {
                self.state.trace.seal.crc_us = self
                    .state
                    .trace
                    .seal
                    .crc_us
                    .wrapping_add(self.file.trace_micros().wrapping_sub(crc_started));
            }
            #[cfg(feature = "page-turn-trace")]
            let dir_started = self.file.trace_micros();
            let dir_ok = self.file.write(payload + 4 * block, &entry, self.file.bytes() == payload + 4 * block);
            #[cfg(feature = "page-turn-trace")]
            {
                self.state.trace.seal.dir_us = self
                    .state
                    .trace
                    .seal
                    .dir_us
                    .wrapping_add(self.file.trace_micros().wrapping_sub(dir_started));
            }
            if !dir_ok {
                return false;
            }
            at += take;
        }
        if whole != self.state.crc {
            return self.file.settle(false);
        }
        let mut manifest = [0; MANIFEST_BYTES as usize];
        envelope::manifest(
            &mut manifest,
            payload,
            &route,
            directory ^ u32::MAX,
            whole ^ u32::MAX,
        );
        #[cfg(feature = "page-turn-trace")]
        let manifest_started = self.file.trace_micros();
        let manifest_ok = self.file.write(payload + 4 * count, &manifest, self.file.bytes() == payload + 4 * count);
        #[cfg(feature = "page-turn-trace")]
        {
            self.state.trace.seal.dir_us = self
                .state
                .trace
                .seal
                .dir_us
                .wrapping_add(self.file.trace_micros().wrapping_sub(manifest_started));
        }
        if !manifest_ok {
            return false;
        }
        #[cfg(feature = "page-turn-trace")]
        let sync_started = self.file.trace_micros();
        let synced = self.file.sync();
        #[cfg(feature = "page-turn-trace")]
        {
            self.state.trace.seal.sync_us = self.file.trace_micros().wrapping_sub(sync_started);
        }
        if !synced {
            return false;
        }
        #[cfg(feature = "page-turn-trace")]
        let verify_started = self.file.trace_micros();
        let verified = self.verify_page_payload(&route, payload, scratch.as_mut(), None);
        #[cfg(feature = "page-turn-trace")]
        {
            self.state.trace.seal.verify_us = self.file.trace_micros().wrapping_sub(verify_started);
        }
        self.file.settle(verified)
    }
    fn begin_cache_write(&mut self, name: &[u8], extent: u32) -> bool {
        self.state.last_published = false;
        self.forget_absent_name(name);
        let lease = self.reset();
        if self.file.generation() != lease {
            return false;
        }
        let Some(text) = valid_name(name) else {
            return false;
        };
        if !self.file.name(text, false, cache_folder(Some(name))) {
            return false;
        }
        if self.file.producing() {
            let page = text.len() == 69 && hex(&text.as_bytes()[..64]) && text.ends_with(".fibp");
            // write() and finalize_page() spend the credit; one census entry
            // per file, temporary included.
            if !page || self.file.write_credit() == 0 || self.state.entries == 0 {
                return false;
            }
            self.state.entries -= 1;
        }
        if !self.file.begin(extent, extent != 0) {
            return false;
        }
        self.state.crc = u32::MAX;
        #[cfg(feature = "page-turn-trace")]
        {
            self.state.trace = state::SegmentTraceState {
                started_us: self.file.trace_micros(),
                ..state::SegmentTraceState::default()
            };
        }
        true
    }
    /// Reserve payload and envelope together; physical size stays fixed while
    /// logical payload grows, so publication never exposes an allocated tail.
    fn begin_index_write(&mut self, name: &[u8], payload: u32) -> bool {
        if payload == 0 || payload > MAX_BYTES || !newt_core::book_index::FileName::parse(name)
            .is_some_and(|file| file.kind() == newt_core::book_index::FileKind::Index) {
            return false;
        }
        self.begin_cache_write(name, payload + 4 * envelope::blocks(payload) + MANIFEST_BYTES)
    }
    /// The attempt owns the cursor; a cache read-back invalidates it.
    fn write_indexed(&mut self, bytes: &[u8], position: &mut Option<u32>) -> bool {
        let payload = self.state.payload;
        if bytes.len() > MAX_BYTES.wrapping_sub(payload) as usize || !self.spend(bytes.len() as u64) {
            return self.file.poison();
        }
        if !self.file.write_indexed(payload, bytes, self.file.bytes() == payload, position) { return false; }
        self.state.crc = envelope::crc(self.state.crc, bytes);
        self.state.payload += bytes.len() as u32;
        true
    }
    /// Reads the complete wire. A damaged committed candidate is an optional
    /// miss, so production repairs it; finalization settles writer failure.
    fn verify_page_payload(
        &mut self,
        route: &envelope::Route,
        payload: u32,
        scratch: &mut [u8],
        cancel: Option<&dyn Authority>,
    ) -> bool {
        self.state.lookup = Lookup::Invalid;
        let mut manifest = [0; MANIFEST_BYTES as usize];
        let extent = self.file.bytes();
        if !self
            .file
            .read(extent.wrapping_sub(MANIFEST_BYTES), &mut manifest)
        {
            self.state.lookup = Lookup::Unknown;
            return false;
        }
        let mut checked = 0;
        if !envelope::inspect_for(self.envelope_name(), &manifest, extent, route, &mut checked)
            || checked != payload
        {
            return false;
        }
        let (mut whole, mut directory) = (u32::MAX, u32::MAX);
        let mut at = 0;
        for block in 0..envelope::blocks(payload) {
            let take = BLOCK_BYTES.min(payload - at);
            let data = &mut scratch.as_mut()[..take as usize];
            let mut entry = [0; 4];
            if !self.file.read(at, data) || !self.file.read(payload + 4 * block, &mut entry) {
                self.state.lookup = Lookup::Unknown;
                return false;
            }
            let Some(checked_block) = self.readback_crc(route, block, data, &mut whole, cancel)
            else {
                self.state.lookup = Lookup::Unknown;
                return false;
            };
            if u32::from_le_bytes(entry) != checked_block {
                return false;
            }
            directory = envelope::crc(directory, &entry);
            if !self.file.allowed() {
                self.state.lookup = Lookup::Unknown;
                return false;
            }
            at += take;
        }
        if (self.file.writing() && whole != self.state.crc)
            || whole ^ u32::MAX != get32(&manifest[24..])
            || directory ^ u32::MAX != get32(&manifest[20..])
        {
            return false;
        }
        if !self.file.allowed() {
            self.state.lookup = Lookup::Unknown;
            return false;
        }
        true
    }
    /// One block's checked CRC, advancing `whole`. None is interrupted work.
    fn readback_crc(
        &mut self,
        route: &envelope::Route,
        block: u32,
        data: &[u8],
        whole: &mut u32,
        cancel: Option<&dyn Authority>,
    ) -> Option<u32> {
        let mut hash = envelope::block_seed(route, block, data.len() as u32);
        for chunk in data.chunks(1024) {
            if self.interrupted(cancel) {
                return None;
            }
            *whole = envelope::crc(*whole, chunk);
            if self.interrupted(cancel) {
                return None;
            }
            hash = envelope::crc(hash, chunk);
            if self.interrupted(cancel) {
                return None;
            }
        }
        Some(hash ^ u32::MAX)
    }
    fn interrupted(&mut self, cancel: Option<&dyn Authority>) -> bool {
        !self.file.allowed() || cancel.is_some_and(|c| c.cancelled())
    }
    fn forget_absent_name(&mut self, name: &[u8]) {
        let identity = name_identity(name);
        for entry in &mut self.state.absent {
            if *entry == identity {
                *entry = 0;
            }
        }
    }
    /// A pre-#179 record may still sit at its old Pages/ name. Only a
    /// confirmed miss in Meta/ (canonical and backup) names it there, so a
    /// damaged Meta/ file stays a miss and an older value never shadows it.
    fn legacy_name(&mut self, name: &str) -> bool {
        self.state.legacy =
            self.envelope_name().is_none() && self.file.absent() && self.file.name(name, false, None);
        self.state.legacy
    }
    /// Copies a validated legacy record into Meta/ when its read ends outside
    /// any checked scope, through the ordinary transactional writer. The
    /// legacy name stays for the census; a failed copy is adopted again later.
    /// end_write never runs this: a writer holds no read or legacy name.
    fn adopt_legacy(&mut self) {
        // The largest record is a 1,024-spine semantic table (4,424 bytes).
        const ADOPT_BYTES: u32 = 8192;
        const NAME_BYTES: usize = 128;
        if !self.state.legacy || !self.state.read_active || self.file.checks_deferred() {
            return;
        }
        if self.file.producing() {
            return;
        }
        self.state.legacy = false;
        let bytes = self.state.payload;
        if bytes > ADOPT_BYTES {
            return;
        }
        let Some(mut copy) = Bytes::try_new(NAME_BYTES + bytes as usize) else {
            return;
        };
        let (name, record) = copy.as_mut().split_at_mut(NAME_BYTES);
        name.fill(0);
        let suffix = self.file.suffix().as_bytes();
        if suffix.len() >= NAME_BYTES {
            return;
        }
        name[..suffix.len()].copy_from_slice(suffix);
        if !self.file.read(0, record) {
            return;
        }
        self.reset();
        if self.begin_write(name) && self.write(record) {
            let _ = self.end_write();
        }
        self.reset();
    }
}

impl<S: Native> CacheStorage for ChapterCacheAccess<'_, '_, S> {
    fn exists(&mut self, name: &[u8]) -> bool {
        ChapterCacheAccess::exists(self, name)
    }
    fn remove(&mut self, _name: &[u8]) -> bool {
        self.close();
        true
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        self.state.lookup = Lookup::Unknown;
        if self.file.writing() {
            return -1;
        }
        // Build a bounded candidate without disturbing the active read lease.
        let Some(text) = valid_name(name) else {
            return -1;
        };
        // Every cache write in a session goes through this storage, so a name
        // proven absent stays absent until it is written or forgotten.
        let identity = name_identity(name);
        if self.state.absent.contains(&identity) {
            return -1;
        }
        if self.state.read_active && self.same(name) {
            let lease = self.file.generation();
            if self.file.stable(lease) {
                return i64::from(self.state.payload);
            }
            self.file.settle(false);
            if self.file.generation() == lease {
                self.file.release();
            }
            // Latch the failed generation: no consumer may silently attach to
            // a replacement same-name file. close() is the explicit boundary.
            return -1;
        }
        let mut lease = self.reset();
        if self.file.generation() != lease {
            return -1;
        }
        let named = self.file.name(text, false, cache_folder(Some(name)));
        let maximum = if self.envelope_name().is_some() {
            MAX_EXTENT
        } else {
            MAX_BYTES + FOOTER_BYTES
        };
        let mut opened = named && self.file.open_read(maximum);
        if named && !opened && self.legacy_name(text) {
            lease = self.file.generation();
            opened = self.file.open_read(MAX_BYTES + FOOTER_BYTES);
        }
        if !opened || !self.validate() {
            // Only this lookup's own open may prove absence; it already released the port.
            let absent = named && !opened && self.file.absent();
            let invalid = named && !opened && self.file.invalid();
            if !absent && !invalid && self.state.lookup != Lookup::Invalid {
                self.file.settle(false);
            }
            if self.file.generation() == lease {
                self.close();
            }
            if absent {
                // Canonical and recovery names are both missing.
                self.state.lookup = Lookup::Missing;
                self.state.absent[usize::from(self.state.next_absent)] = identity;
                self.state.next_absent = (self.state.next_absent + 1) % 3;
            } else if invalid {
                self.state.lookup = Lookup::Invalid;
            }
            return -1;
        }
        self.state.read_active = true;
        i64::from(self.state.payload)
    }
    fn read_at(&mut self, name: &[u8], offset: u32, dst: &mut [u8]) -> i32 {
        if !self.file.port.route.cache_read_live() {
            return -1;
        }
        // file_size sets the payload this bound reads.
        if self.file_size(name) < 0
            || self.file.envelope_read()
            || offset > self.state.payload
            || dst.len() > (self.state.payload - offset) as usize
        {
            return -1;
        }
        let lease = self.file.generation();
        if self.file.read(offset, dst) && self.file.port.route.cache_read_live() {
            return dst.len() as i32;
        }
        self.file.settle(false);
        if self.file.generation() == lease {
            self.file.release();
        }
        -1
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.begin_cache_write(name, 0)
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        let payload = self.state.payload;
        if bytes.len() > MAX_BYTES.wrapping_sub(payload) as usize || !self.spend(bytes.len() as u64)
        {
            return self.file.poison();
        }
        #[cfg(feature = "page-turn-trace")]
        let started = self.file.trace_micros();
        if !self.file.write(payload, bytes, self.file.bytes() == payload) {
            return false;
        }
        if self.file.producing() {
            // Producer admission has no foreground read budget; poll at 1 KiB.
            for chunk in bytes.chunks(1024) {
                if !self.file.allowed() {
                    return self.file.poison();
                }
                self.state.crc = envelope::crc(self.state.crc, chunk);
                if !self.file.allowed() {
                    return self.file.poison();
                }
            }
        } else {
            self.state.crc = envelope::crc(self.state.crc, bytes);
        }
        #[cfg(feature = "page-turn-trace")]
        {
            self.state.trace.seal.write_us = self
                .state
                .trace
                .seal
                .write_us
                .wrapping_add(self.file.trace_micros().wrapping_sub(started));
        }
        self.state.payload += bytes.len() as u32;
        true
    }
    fn read_back_at(&mut self, offset: u32, dst: &mut [u8]) -> i32 {
        let payload = self.state.payload;
        if !self.file.writing() || offset > payload || dst.len() > (payload - offset) as usize {
            return -1;
        }
        if self.file.read(offset, dst) {
            dst.len() as i32
        } else {
            -1
        }
    }
    fn end_write_checked(&mut self, receipt_live: &mut dyn FnMut() -> bool) -> bool {
        let state = &mut *self.state;
        self.file.with_receipt_check(receipt_live, |file|
            ChapterCacheAccess { file, state }.end_write())
    }
    fn end_write(&mut self) -> bool {
        self.state.last_published = false;
        if !self.file.writing() {
            return false;
        }
        let lease = self.file.generation();
        let committed = if self.envelope_name().is_some() {
            #[cfg(feature = "page-turn-trace")]
            let fin_started = self.file.trace_micros();
            let finalized = self.finalize_page();
            #[cfg(feature = "page-turn-trace")]
            {
                self.state.trace.seal.finalize_us =
                    self.file.trace_micros().wrapping_sub(fin_started);
            }
            if finalized {
                #[cfg(feature = "page-turn-trace")]
                let commit_started = self.file.trace_micros();
                let committed = self.file.commit(MAX_EXTENT, false);
                #[cfg(feature = "page-turn-trace")]
                {
                    self.state.trace.seal.commit_us =
                        self.file.trace_micros().wrapping_sub(commit_started);
                }
                committed
            } else {
                false
            }
        } else {
            let mut footer = [0; FOOTER_BYTES as usize];
            footer[..4].copy_from_slice(&(self.state.crc ^ u32::MAX).to_le_bytes());
            footer[4..8].copy_from_slice(&self.state.payload.to_le_bytes());
            footer[8..].copy_from_slice(b"NTCC");
            if !self.file.producing() {
                self.file.charge_write(u64::from(FOOTER_BYTES));
            }
            self.file.write(self.state.payload, &footer, true)
                && self.file.sync()
                && self.validate()
                && self.file.commit(MAX_BYTES + FOOTER_BYTES, false)
        };
        #[cfg(feature = "page-turn-trace")]
        {
            self.state.trace.bytes = self.state.payload;
        }
        self.state.last_published = committed || self.file.promoted();
        if !committed {
            self.file.settle(false);
            // Not close(): see adopt_legacy.
            if self.file.generation() == lease {
                self.reset();
            }
            return false;
        }
        self.state.read_active = false;
        true
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_micros(&self) -> u32 {
        self.file.trace_micros()
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_begin(&mut self, idx: u32) {
        self.state.trace.idx = idx;
        self.state.trace.active = true;
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_add_tee_us(&mut self, us: u32) {
        self.state.trace.tee_us = self.state.trace.tee_us.wrapping_add(us);
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_layout_done(&mut self) {
        let wall = self
            .file
            .trace_micros()
            .wrapping_sub(self.state.trace.started_us);
        self.state.trace.layout_us = wall.saturating_sub(self.state.trace.tee_us);
    }
    #[cfg(feature = "page-turn-trace")]
    fn trace_segment_end(&mut self, pages: u32) {
        if self.state.trace.active {
            self.state.trace.active = false;
            self.file.emit_segment(newt_core::page_turn_trace::SegmentTrace {
                idx: self.state.trace.idx,
                pages,
                bytes: self.state.trace.bytes,
                layout_us: self.state.trace.layout_us,
                tee_us: self.state.trace.tee_us,
                seal: self.state.trace.seal,
            });
        }
    }
    fn last_write_published(&self) -> bool {
        self.state.last_published
    }
    fn producer_can_begin(&self) -> bool {
        !self.file.producing()
            || (self.state.entries > 0
                && self.file.write_credit() >= u64::from(newt_core::cache_names::namespace::PAGE_CANDIDATE))
    }
    fn producer_remaining_credit(&self) -> Option<u64> {
        self.file.producing().then(|| self.file.write_credit())
    }
}

/// The original's query read callback: the retained descriptor, its
/// generation and name, inside a live immutable cache-read scope.
impl<S: Native> envelope::RawRead for ChapterCacheAccess<'_, '_, S> {
    fn read(&mut self, query: &Query<'_>, offset: u32, out: &mut [u8]) -> bool {
        query.active
            && self.file.immutable_read()
            && self.file.generation() == query.generation
            && self.file.envelope_read()
            && self.same(query.name.unwrap_or_default())
            && self.file.port.route.cache_read_live()
            && self.file.read(offset, out)
            && self.file.generation() == query.generation
            && self.file.port.route.cache_read_live()
    }
}

include!("chapter_cache_name.rs");

/// Folded 32-bit FNV-1a, never zero: 16 bits keep the original owner's size.
fn name_identity(name: &[u8]) -> u16 {
    let hash = c_str(name).iter().fold(2_166_136_261u32, |hash, &b| {
        (hash ^ u32::from(b)).wrapping_mul(16_777_619)
    });
    match (hash ^ (hash >> 16)) as u16 {
        0 => 1,
        folded => folded,
    }
}

fn get32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[path = "chapter_cache_census.rs"]
mod census;

#[path = "index_build.rs"]
mod index_build;
pub use index_build::{IndexBuildStore, IndexWorkState};

#[path = "chapter_cache_read_scope.rs"]
mod read_scope;
