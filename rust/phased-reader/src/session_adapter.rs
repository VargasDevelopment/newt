//! MIT. Operation-scoped integration of the actual reader and revision Session.
//! No source/Action is retained; all resource loans end when this adapter drops.
use super::*;
use crate::imported_revision::{Digest, session};
use newt_core::{
    page_cache::{CacheStorage, WriterMemory, layout_generation_hash},
    preferences::ViewSettings,
};

// Use the envelope version for pagination changes that replace Pages in place.
// Bumping this revision changes the filenames and leaves the old files behind.
const CACHE_ENGINE_REVISION: u32 = 4;

/// Borrow this view for one Session operation, then return every resource loan.
/// The actual reader (including both selections and its plan) survives the view.
pub struct OperationReader<'op, 'sel, 'plan, 'mem, 'style, 'arena, B: ParserBackend + ?Sized> {
    pub reader: &'op mut PhasedReader<'sel, 'plan>,
    pub metadata: &'op mut Arena<'arena>,
    pub parse: &'op mut Arena<'arena>,
    pub resources: &'op mut canonical::Resources<'mem, B>,
    pub toc: &'op mut [TocEntry],
    pub style: &'op mut StyleResources<'mem, 'arena>,
    pub sources: &'op mut Sources<'style>,
    pub slot: &'op mut Slot<'style>,
    pub layout: &'op mut LayoutResources<'mem>,
    pub lease: &'op Lease,
    pub(super) staged: Option<(u32, u64, bool, bool)>,
    original_held: bool,
}
impl<'op, 'sel, 'plan, 'mem, 'style, 'arena, B: ParserBackend + ?Sized>
    OperationReader<'op, 'sel, 'plan, 'mem, 'style, 'arena, B>
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reader: &'op mut PhasedReader<'sel, 'plan>,
        metadata: &'op mut Arena<'arena>,
        parse: &'op mut Arena<'arena>,
        resources: &'op mut canonical::Resources<'mem, B>,
        toc: &'op mut [TocEntry],
        style: &'op mut StyleResources<'mem, 'arena>,
        sources: &'op mut Sources<'style>,
        slot: &'op mut Slot<'style>,
        layout: &'op mut LayoutResources<'mem>,
        lease: &'op Lease,
    ) -> Self {
        Self {
            reader,
            metadata,
            parse,
            resources,
            toc,
            style,
            sources,
            slot,
            layout,
            lease,
            staged: None,
            original_held: false,
        }
    }
}
impl<B: ParserBackend + ?Sized> session::Reader for OperationReader<'_, '_, '_, '_, '_, '_, B> {
    fn trace(&self, event: newt_core::admission::WorkEvent) {
        #[cfg(feature = "dev-usb")]
        if let Some(hook) = self.reader.on_yield { hook.observe(event); }
        #[cfg(not(feature = "dev-usb"))]
        let _ = event;
    }
    fn close(&mut self) {
        let _ = session::Reader::finish_original(self);
        self.staged = None;
        self.reader.close();
        self.metadata.reset();
        self.parse.reset();
    }
    fn strict_open(&mut self, source: &mut dyn BookSource) -> Status {
        session::Reader::close(self);
        self.reader.open(
            source,
            self.metadata,
            self.parse,
            self.resources,
            self.toc,
            self.style,
            self.sources,
            self.slot,
            Some(self.lease),
        )
    }
    fn spine_count(&self) -> u32 {
        self.reader.selection().spines
    }
    fn stage(
        &mut self,
        source: &mut dyn BookSource,
        spine: u32,
        plain: bool,
    ) -> Result<Entry, Status> {
        self.staged = None;
        if self.reader.pending.is_some() {
            return Err(Status::Unsupported);
        }
        let bytes = if self.reader.opened {
            self.reader.source_bytes
        } else {
            source.size()
        };
        let status = validate(source, bytes);
        if status != Status::Ok {
            return Err(status);
        }
        // Session owns the outer ParseLease here. Do not acquire it twice.
        let result = select_mode(
            source,
            spine,
            &mut self.reader.selections[self.reader.active ^ 1],
            self.metadata,
            self.resources,
            self.toc,
            self.style,
            self.sources,
            self.slot,
            self.reader.on_yield,
            self.reader.work,
            plain,
        );
        self.metadata.reset();
        self.parse.reset();
        let undeclared = result?;
        let status = validate(source, bytes);
        if status != Status::Ok {
            return Err(status);
        }
        self.staged = Some((spine, bytes, undeclared, plain));
        Ok(self.reader.staged_selection().entry)
    }
    fn acquire_original(&mut self) -> Result<(), Status> {
        if self.original_held {
            return Err(Status::Unsupported);
        }
        if !(self.lease.acquire)() {
            return Err(Status::OutOfMemory);
        }
        self.original_held = true;
        Ok(())
    }
    fn finish_original(&mut self) -> Result<(), Status> {
        if self.original_held {
            self.original_held = false;
            (self.lease.returned)();
        }
        Ok(())
    }
    fn commit(&mut self, spine: u32) {
        // Session calls only after verifying this returned Entry, withdrawing
        // Action, returning ParseLease and checking owner/source generation.
        let Some((staged, bytes, undeclared, plain)) = self.staged.take() else {
            return;
        };
        if staged != spine {
            return;
        }
        let r = &mut self.reader;
        r.active ^= 1;
        r.source_bytes = bytes;
        r.opened = true;
        r.info.spine = spine;
        r.proofs.selection = true;
        r.styles_ready = !plain && self.slot.state != StyleState::Empty;
        r.undeclared_styles = undeclared;
        // No page/anchor/character totals, frame or plan are fabricated.
    }
}

impl<B: ParserBackend + ?Sized> Drop for OperationReader<'_, '_, '_, '_, '_, '_, B> {
    fn drop(&mut self) {
        if self.original_held {
            self.original_held = false;
            (self.lease.returned)();
        }
    }
}

/// Separate operation loan; never captures the reader, source or Action.
pub struct OperationCache<'op, 'mem, C> {
    pub store: &'op mut C,
    pub memory: &'op mut WriterMemory<'mem>,
    pub hash: &'op mut dyn crate::hash_provider::CacheHash,
    pub settings: ViewSettings,
}
impl<B: ParserBackend + ?Sized, C: CacheStorage + CachePresence>
    session::Cache<OperationReader<'_, '_, '_, '_, '_, '_, B>> for OperationCache<'_, '_, C>
{
    fn build(
        &mut self,
        source: &mut dyn BookSource,
        digest: &Digest,
        spine: u32,
        r: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>,
    ) -> session::Build {
        // The cache must match the reader's page layout (orientation and text
        // size). Refuse other view choices instead of routing pages under
        // settings the reader never received.
        if !reader_settings_match(r.reader, self.settings) {
            return session::Build::Failed(Status::Unsupported);
        }
        let Some(language) = r.reader.book_language() else {
            return session::Build::Failed(Status::NotFound);
        };
        let (name, generation) = match identity(self.settings, digest, spine, self.hash, language) {
            Ok(v) => v,
            Err(e) => return session::Build::Failed(e),
        };
        let result = CacheBuild {
            name: &name,
            generation,
        }
        .build(
            r.reader,
            source,
            spine,
            self.store,
            self.memory,
            r.metadata,
            r.parse,
            r.resources,
            r.toc,
            r.style,
            r.sources,
            r.slot,
            r.layout,
            r.lease,
        );
        if result.verified_empty {
            session::Build::Empty
        } else if result.status == Status::Ok {
            session::Build::Ready
        } else {
            session::Build::Failed(result.status)
        }
    }
    fn stored(
        &mut self,
        digest: &Digest,
        spine: u32,
        r: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>,
    ) -> bool {
        let _action = newt_core::admission::ActionScope::new(r.reader.on_yield,
            newt_core::reader_action_profile::ActionStage::CacheQuery);
        // Typed plan and lease loans guard cache availability.
        // No opened/source-good requirement: this is only a presence hint.
        if r.reader.plan.is_none() || !reader_settings_match(r.reader, self.settings) {
            return false;
        }
        let opening = !r.reader.has_page();
        cached_for_reader(r.reader, self.settings, digest, spine, self.hash, &mut |name| {
            self.store.present(name, opening)
        })
    }
}

/// Chapter page cache identity: SHA-256 over exactly 99 bytes.
/// The hash loan is used only for this call; packageSha256 failure is observable.
pub(super) fn identity(
    settings: ViewSettings,
    digest: &Digest,
    spine: u32,
    hash: &mut dyn crate::hash_provider::CacheHash,
    language: BookLanguage,
) -> Result<([u8; 69], u32), Status> {
    let generation = identity_generation(settings, language);
    identity_at_generation(settings, digest, spine, hash, generation).map(|name| (name, generation))
}
fn identity_generation(settings: ViewSettings, language: BookLanguage) -> u32 {
    let params = reader_parameters(settings.page_layout(), None, language);
    let mix = |h: u32, v: u32| (h ^ v).wrapping_mul(16777619);
    let engine = CACHE_ENGINE_REVISION;
    let mut generation = layout_generation_hash(&params, newt_reader_atlas::IDENTITY, false);
    generation = mix(generation, engine);
    generation = mix(generation, styles::ID);
    generation = mix(
        mix(generation, u32::from(settings.slim_progress_bar)),
        u32::from(settings.refresh_pages),
    );
    generation
}
fn identity_at_generation(
    settings: ViewSettings, digest: &Digest, spine: u32, hash: &mut dyn crate::hash_provider::CacheHash, generation: u32,
) -> Result<[u8; 69], Status> {
    let font = b"ce2bda740b0031ba41b864e73c9d1107e4c59383b8ae1a64b16fd3fa9d91f553";
    let sdk = b"cd6f5b831c8fad899723b6d406586f00c575f021";
    let unhex = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
    let mut encoded = [0; 99];
    encoded[..32].copy_from_slice(digest);
    for i in 0..32 {
        encoded[32 + i] = (unhex(font[2 * i]) << 4) | unhex(font[2 * i + 1]);
    }
    for i in 0..20 {
        encoded[64 + i] = (unhex(sdk[2 * i]) << 4) | unhex(sdk[2 * i + 1]);
    }
    for (at, value) in [(84, spine), (88, generation), (92, CACHE_ENGINE_REVISION)] {
        encoded[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    // The layout class, not the turn: both landscapes share their pages.
    encoded[96] = u8::from(settings.orientation.is_landscape());
    encoded[97] = u8::from(settings.slim_progress_bar);
    encoded[98] = settings.refresh_pages;
    let mut key = [0; 32];
    if !hash.hash(&encoded, &mut key) {
        return Err(Status::IoError);
    }
    let mut name = [0; 69];
    for (i, b) in key.iter().enumerate() {
        name[2 * i] = b"0123456789abcdef"[(b >> 4) as usize];
        name[2 * i + 1] = b"0123456789abcdef"[(b & 15) as usize];
    }
    name[64..].copy_from_slice(b".fibp");
    Ok(name)
}

/// Presence deliberately does not pass through CacheBuild's source-good store.
/// Implement with actual exists, or an existing native scoped presence callback.
/// A successful hint does not certify file contents or immutable source bytes.
pub trait CachePresence {
    fn present(&mut self, name: &[u8], opening: bool) -> bool;
}

pub(super) fn reader_settings_match(reader: &PhasedReader<'_, '_>, settings: ViewSettings) -> bool {
    let defaults = ViewSettings::default();
    reader.page_layout == settings.page_layout()
        && settings.slim_progress_bar == defaults.slim_progress_bar
        && settings.refresh_pages == defaults.refresh_pages
}

/// Presence hint before OPF selection on warm open or resume-picture lookup.
/// A positive hint still requires verified selection before a cache read.
pub fn cached(
    settings: ViewSettings,
    digest: &Digest,
    spine: u32,
    hash: &mut dyn crate::hash_provider::CacheHash,
    present: &mut dyn FnMut(&[u8]) -> bool,
) -> bool {
    BookLanguage::ALL.into_iter()
        .any(|language| cached_for_language(settings, digest, spine, hash, language, present))
}

pub(super) fn cached_for_reader(
    reader: &PhasedReader<'_, '_>,
    settings: ViewSettings,
    digest: &Digest,
    spine: u32,
    hash: &mut dyn crate::hash_provider::CacheHash,
    present: &mut dyn FnMut(&[u8]) -> bool,
) -> bool {
    if let Some(language) = reader.book_language() {
        cached_for_language(settings, digest, spine, hash, language, present)
    } else {
        cached(settings, digest, spine, hash, present)
    }
}

/// Probe the map only after a base miss.
pub(super) fn cached_for_language(
    settings: ViewSettings,
    digest: &Digest,
    spine: u32,
    hash: &mut dyn crate::hash_provider::CacheHash,
    language: BookLanguage,
    present: &mut dyn FnMut(&[u8]) -> bool,
) -> bool {
    let Ok((name, _)) = identity(settings, digest, spine, hash, language) else {
        return false;
    };
    if present(&name) {
        return true;
    }
    let mut map = [0; 128];
    map[..name.len()].copy_from_slice(&name);
    if !newt_core::cache_names::segment::map_name(&mut map, &mut |input, out| hash.hash(input, out))
    {
        return false;
    }
    present(&map[..69])
}

/// Legacy/no-authority presence over the actual store's raw exists callback.
/// This path deliberately has no native scope; opening is unused as in presentIn.
/// A target with real native authority can implement CachePresence separately.
pub struct PresenceStore<'a, C, F> {
    pub store: &'a mut C,
    pub exists: F,
}
impl<C, F: FnMut(&mut C, &[u8]) -> bool> CachePresence for PresenceStore<'_, C, F> {
    fn present(&mut self, name: &[u8], _opening: bool) -> bool {
        (self.exists)(self.store, name)
    }
}
impl<C: CacheStorage, F> CacheStorage for PresenceStore<'_, C, F> {
    fn remove(&mut self, name: &[u8]) -> bool {
        self.store.remove(name)
    }
    fn file_size(&mut self, name: &[u8]) -> i64 {
        self.store.file_size(name)
    }
    fn read_at(&mut self, name: &[u8], at: u32, dst: &mut [u8]) -> i32 {
        self.store.read_at(name, at, dst)
    }
    fn begin_write(&mut self, name: &[u8]) -> bool {
        self.store.begin_write(name)
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.store.write(bytes)
    }
    fn end_write(&mut self) -> bool {
        self.store.end_write()
    }
    fn read_back_at(&mut self, at: u32, dst: &mut [u8]) -> i32 {
        self.store.read_back_at(at, dst)
    }
}

#[cfg(test)]
#[path = "presence_cases.rs"]
pub(crate) mod presence_cases;

#[path = "producer_segments.rs"]
pub mod producer;

#[cfg(all(test, feature = "native-host"))]
#[allow(unsafe_code)]
#[path = "producer_stream_cases.rs"]
mod producer_stream_tests;
