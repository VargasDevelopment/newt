//! Bounded index construction. Source parsing ends before publication begins.
#![forbid(unsafe_code)]
use crate::{
    book_index::{Header, Identity, Member, NONE, MAX_MEMBERS, MAX_SPINE, WriteError, name_hash},
    member_check::Checked, package_parsers::Workflow, page_cache::CacheStorage,
    phased_metadata::{BookLanguage, copy_display_title},
    xml_input::{TextCapture, has_token, local_is, resolve_href_checked, HrefError},
    xml_session::{Attributes, Handler, ParserBackend}, zip_container::Status,
    zip_window::{arena::Arena, source::BookSource},
};
mod runs;
mod records;
mod engine;
use engine::Engine;
use records::{Item, Reference};
pub use engine::POSITION_WORDS;
pub use runs::{Cursor, CURSORS, Statistics};
const TEXT: u32 = 1;
const NAV: u32 = 2;
const NCX: u32 = 4;
const COVER: u32 = 8;
const CSS: u32 = 16;

/// Append and read share one descriptor. A successful read fills the whole slice.
/// Removal must close the descriptor too. No operation publishes an index.
pub trait WorkFile {
    fn append(&mut self, bytes: &[u8]) -> bool;
    fn read_at(&mut self, offset: u32, out: &mut [u8]) -> bool;
    fn remove_work(&mut self) -> bool;
    /// Surface delayed write errors before the index can be published.
    fn sync_work(&mut self) -> bool;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Join pairs manifest items with spine references; Rank walks members in name order.
pub enum Phase { Central, Package, Join, Merge, Rank, Publish }
/// `done` never falls within one phase; Merge counts work-file bytes across the whole build.
/// `total == 0` means that the pass is streaming and its total is unknown.
#[derive(Clone, Copy, Debug)]
pub struct Progress { pub phase: Phase, pub done: u64, pub total: u64 }
#[derive(Debug, PartialEq, Eq)]
pub enum Error { Book(Status), Write(WriteError) }
impl Error {
    fn status(self) -> Status { match self { Self::Book(s) => s, Self::Write(_) => Status::IoError } }
}
#[derive(Clone, Copy)]
struct Choice { sequence: u32, hash: u64 }
fn earlier(target: &mut Option<Choice>, candidate: Choice) {
    if target.is_none_or(|old| candidate.sequence < old.sequence) { *target = Some(candidate); }
}

/// Writer::begin borrows this prefix of the run buffer; publish saves it in JoinState.
pub const WRITER_PREFIX: usize = crate::book_index::HEADER_BYTES as usize;
/// Exact bytes a JoinState borrows, before alignment padding.
pub const JOIN_STATE_BYTES: usize = core::mem::size_of::<[u64; POSITION_WORDS]>()
    + core::mem::size_of::<[Cursor; CURSORS]>() + WRITER_PREFIX;
/// The most builder state any phase keeps on the caller's stack. The OPF pass
/// holds the builder and the package handler; join and publish hold the engine
/// and their merge handles.
pub const STACK_BYTES: usize = {
    let package = core::mem::size_of::<Builder<'static>>()
        + core::mem::size_of::<Package<'static, 'static, dyn WorkFile>>();
    if package > engine::JOIN_STACK_BYTES { package } else { engine::JOIN_STACK_BYTES }
};
// Both budgets sit just above today's host sizes (7,552 B and 2,264 B), so
// any growth is a decision, not a silent stack overflow or bank overrun: the
// reader task had about 6.1 KB of stack free on a normal open (SLICE0).
const _: () = assert!(JOIN_STATE_BYTES <= 7_552, "JoinState outgrew its parse-bank share");
const _: () = assert!(STACK_BYTES <= 2_304, "index builder stack state outgrew its budget");

/// Merge state for join and publish, lent from the parse bank once the OPF
/// pass has released inflate and XML: the joined-position set, the cursor
/// pool and the writer's saved prefix. Lent in pieces so the caller can carve
/// each in place.
pub struct JoinState<'p> {
    positions: &'p mut [u64; POSITION_WORDS],
    cursors: &'p mut [Cursor; CURSORS],
    saved: &'p mut [u8; WRITER_PREFIX],
}
impl<'p> JoinState<'p> {
    pub fn new(positions: &'p mut [u64; POSITION_WORDS], cursors: &'p mut [Cursor; CURSORS],
        saved: &'p mut [u8; WRITER_PREFIX]) -> Self {
        Self { positions, cursors, saved }
    }
}

/// Lent for one build only.
pub struct Builder<'a> { engine: Engine<'a>, members: u32 }
/// The OPF pass is over. Nothing here borrows the parse bank, so the caller
/// can lend that bank as a JoinState.
pub struct Parsed<'a> { engine: Engine<'a>, header: Header, choices: Choices }
pub struct Prepared<'a> { engine: Engine<'a>, header: Header }
/// `work_removed == false` means the index is published but the work file is
/// left for the next census to sweep; the open still succeeds.
pub struct Report { pub header: Header, pub statistics: Statistics, pub work_removed: bool }
/// Navigation and cover candidates the OPF pass found, resolved by the join.
#[derive(Clone, Copy)]
struct Choices { toc: Option<u64>, cover_id: Option<u64>, nav: Option<Choice>, ncx: Option<Choice>, cover: Option<Choice> }
impl<'a> Builder<'a> {
    pub fn new(buffer: &'a mut [u8], min_read: usize, run_bytes: usize) -> Result<Self, Error> {
        Ok(Self { engine: Engine::new(buffer, min_read, run_bytes)?, members: 0 })
    }
    pub fn visit<W: WorkFile + ?Sized>(&mut self, work: &mut W, member: Member,
        progress: &mut dyn FnMut(Progress) -> bool) -> Status {
        let result = (|| {
            if self.members == MAX_MEMBERS { return Err(Error::Book(Status::MetadataTooLarge)); }
            if !progress(Progress { phase: Phase::Central, done: u64::from(self.members), total: 0 }) { return Err(Error::Book(Status::Stale)); }
            self.engine.member(work, member)?;
            self.members += 1;
            Ok(())
        })();
        match result { Ok(()) => Status::Ok, Err(error) => { work.remove_work(); error.status() } }
    }
    /// The OPF pass. It consumes the workflow, so its inflate and XML loans
    /// end here and `Parsed` keeps none of them.
    pub fn parse<S: BookSource + ?Sized, B: ParserBackend + ?Sized, W: WorkFile + ?Sized>(self,
        workflow: Workflow<'_, S, B>, entry: &Checked, dir: &[u8], identity: Identity,
        work: &mut W, progress: &mut dyn FnMut(Progress) -> bool) -> Result<Parsed<'a>, Error> {
        let result = self.parse_inner(workflow, entry, dir, identity, work, progress);
        if result.is_err() { work.remove_work(); }
        result
    }
    fn parse_inner<S: BookSource + ?Sized, B: ParserBackend + ?Sized, W: WorkFile + ?Sized>(mut self,
        mut workflow: Workflow<'_, S, B>, entry: &Checked, dir: &[u8], identity: Identity,
        work: &mut W, progress: &mut dyn FnMut(Progress) -> bool) -> Result<Parsed<'a>, Error> {
        let mut package = Package {
            engine: &mut self.engine, work, progress, path: [0; 512], capture: TextCapture::default(),
            nav: None, ncx: None, image: None, dir,
            metadata_depth: 0, manifest_depth: 0, spine_depth: 0, field: 0,
            title: [0; 256], title_set: false, stylesheets: false, language: BookLanguage::Default,
            language_set: false, toc: None, cover: None, manifests: 0, idrefs: 0,
            raw_items: 0, raw_refs: 0, path_peak: 0, progressed: 0, error: Status::Ok,
        };
        let status = workflow.pass(entry, &mut package);
        if status != Status::Ok { return Err(Error::Book(status)); }
        if package.error != Status::Ok { return Err(Error::Book(package.error)); }
        if package.raw_items == 0 || package.raw_refs == 0 { return Err(Error::Book(Status::NotEpub)); }
        if !package.title_set { copy_display_title(&mut package.title, None); }
        let header = Header { stylesheets: package.stylesheets, navigation_declared: false, identity, members: self.members, spine: 0, spine_bytes: 0,
            nav: NONE, ncx: NONE, cover: NONE, title: package.title, language: package.language };
        let choices = Choices { toc: package.toc, cover_id: package.cover,
            nav: package.nav, ncx: package.ncx, cover: package.image };
        Ok(Parsed { engine: self.engine, header, choices })
    }
}
impl<'a> Parsed<'a> {
    /// Joins manifest, spine and members through the lent merge state.
    pub fn join<W: WorkFile + ?Sized>(mut self, work: &mut W, state: &mut JoinState<'_>,
        progress: &mut dyn FnMut(Progress) -> bool) -> Result<Prepared<'a>, Error> {
        let result = self.engine.join(work, &mut self.header, self.choices, state, progress);
        if let Err(error) = result { work.remove_work(); return Err(error); }
        Ok(Prepared { engine: self.engine, header: self.header })
    }
}
struct Package<'a, 'b, W: WorkFile + ?Sized> {
    engine: &'a mut Engine<'b>,
    work: &'a mut W,
    progress: &'a mut dyn FnMut(Progress) -> bool,
    path: [u8; 512],
    capture: TextCapture,
    nav: Option<Choice>,
    ncx: Option<Choice>,
    image: Option<Choice>,
    dir: &'a [u8],
    metadata_depth: usize,
    manifest_depth: usize,
    spine_depth: usize,
    field: u8,
    title: [u8; 256],
    title_set: bool,
    stylesheets: bool,
    language: BookLanguage,
    language_set: bool,
    toc: Option<u64>,
    cover: Option<u64>,
    manifests: usize,
    idrefs: usize,
    raw_items: usize,
    raw_refs: usize,
    path_peak: usize,
    progressed: u64,
    error: Status,
}
impl<W: WorkFile + ?Sized> Package<'_, '_, W> {
    fn fail(&mut self, status: Status) {
        if self.error == Status::Ok { self.error = status; }
    }
    fn item(&mut self, atts: &mut dyn Attributes) {
        self.raw_items += 1;
        let (Some(id), Some(href)) = (atts.local(b"id"), atts.local(b"href")) else { return; };
        if self.manifests == MAX_MEMBERS as usize {
            self.fail(Status::MetadataTooLarge);
            return;
        }
        let mut arena = Arena::new(&mut self.path);
        let resolved = resolve_href_checked(&mut arena, Some(self.dir), Some(href), None);
        self.path_peak = self.path_peak.max(arena.used());
        let hash = match resolved.and_then(|path| arena.view(path).ok_or(HrefError::Capacity)) {
            Ok(path) => name_hash(path),
            Err(error) => {
                self.fail(match error {
                    HrefError::TooLong => Status::PathTooLong,
                    HrefError::Capacity => Status::MetadataTooLarge,
                    HrefError::Missing => Status::ParseError,
                });
                return;
            }
        };
        let media = atts.local(b"media-type");
        let mut flags = 0;
        if matches!(media, Some(b"application/xhtml+xml" | b"text/html")) { flags |= TEXT; }
        if media == Some(b"text/css") { flags |= CSS; self.stylesheets = true; }
        if media == Some(b"application/x-dtbncx+xml") { flags |= NCX; }
        if has_token(atts.local(b"properties"), b"nav") { flags |= NAV; }
        if has_token(atts.local(b"properties"), b"cover-image") { flags |= COVER; }
        let sequence = self.manifests as u32;
        let choice = Choice { sequence, hash };
        if flags & NAV != 0 && self.nav.is_none() { self.nav = Some(choice); }
        if flags & NCX != 0 && self.ncx.is_none() { self.ncx = Some(choice); }
        if flags & COVER != 0 && self.image.is_none() { self.image = Some(choice); }
        if let Err(error) = self.engine.item(self.work, Item {
            id: name_hash(id), href: hash, sequence, flags,
        }) { self.fail(error.status()); }
        self.manifests += 1;
    }
}
impl<W: WorkFile + ?Sized> Handler for Package<'_, '_, W> {
    fn start_element(&mut self, name: &[u8], atts: &mut dyn Attributes) {
        if self.error != Status::Ok { return; }
        self.progressed += 1;
        if !(self.progress)(Progress { phase: Phase::Package, done: self.progressed, total: 0 }) {
            self.fail(Status::Stale); return;
        }
        if local_is(name, b"metadata") { self.metadata_depth += 1; }
        else if local_is(name, b"manifest") { self.manifest_depth += 1; }
        else if local_is(name, b"spine") {
            self.spine_depth += 1;
            if self.toc.is_none() { self.toc = atts.local(b"toc").map(name_hash); }
        } else if self.metadata_depth > 0 && local_is(name, b"meta") {
            if atts.local(b"name") == Some(b"cover") && let Some(id) = atts.local(b"content") {
                self.cover = Some(name_hash(id));
            }
        } else if self.metadata_depth > 0 {
            self.field = if local_is(name, b"title") { 1 }
                else if local_is(name, b"language") { 2 } else { 0 };
            if self.field != 0 { self.capture.begin(); }
        } else if self.manifest_depth > 0 && local_is(name, b"item") { self.item(atts); }
        else if self.spine_depth > 0 && local_is(name, b"itemref") {
            self.raw_refs += 1;
            if let Some(id) = atts.local(b"idref") {
                if self.idrefs == MAX_SPINE as usize { self.fail(Status::MetadataTooLarge); }
                else {
                    if let Err(error) = self.engine.reference(self.work, Reference {
                        id: name_hash(id), position: self.idrefs as u32,
                    }) { self.fail(error.status()); }
                    self.idrefs += 1;
                }
            }
        }
    }
    fn end_element(&mut self, name: &[u8]) {
        if local_is(name, b"metadata") { self.metadata_depth = self.metadata_depth.saturating_sub(1); }
        else if local_is(name, b"manifest") { self.manifest_depth = self.manifest_depth.saturating_sub(1); }
        else if local_is(name, b"spine") { self.spine_depth = self.spine_depth.saturating_sub(1); }
        else if self.capture.active {
            let text = self.capture.trimmed();
            if !text.is_empty() {
                if self.field == 1 && !self.title_set {
                    copy_display_title(&mut self.title, Some(text));
                    self.title_set = true;
                } else if self.field == 2 && !self.language_set {
                    self.language = BookLanguage::from_opf(Some(text));
                    self.language_set = true;
                }
            }
            self.capture.end();
            self.field = 0;
        }
    }
    fn text(&mut self, text: &[u8]) {
        self.capture.add(text);
        self.progressed += text.len() as u64;
        if !(self.progress)(Progress { phase: Phase::Package, done: self.progressed, total: 0 }) {
            self.fail(Status::Stale);
        }
    }    /// A cancel or a failed spill ends the OPF read at once instead of at its last byte.
    fn stop_parse(&self) -> bool { self.error != Status::Ok }
}


impl Prepared<'_> {
    /// With the source open, publish only a resident build: the work file and
    /// index temporary must not both be open alongside source and receipt.
    /// After an error the unfinished index temp stays with the store; closing the
    /// cache owner (or its next begin_write) removes it, as for any failed cache write.
    /// Once the index is published, a failed work-file removal is reported, not
    /// fatal: like a failed eviction, the census sweeps the file later.
    pub fn publish<W: WorkFile + CacheStorage + ?Sized>(mut self, store: &mut W, state: &mut JoinState<'_>,
        progress: &mut dyn FnMut(Progress) -> bool, receipt_live: &mut dyn FnMut() -> bool) -> Result<Report, Error> {
        let result = self.engine.publish(store, self.header, state, progress, receipt_live);
        let work_removed = store.remove_work();
        result?;
        Ok(Report { header: self.header, statistics: self.engine.io.stats, work_removed })
    }
}

#[cfg(test)]
mod tests;

/// Index envelope plus append-only work for 32 KiB runs and 512-byte reads.
/// Initial bytes are M=24E, I=24I, R=12R; joins add J1=16R,
/// F=8min(E,I,R), J2=12R. I, R and J1 need at most two intermediate
/// generations. M's at most 49 runs and J2's at most 60 fit their read shares.
/// Headers cover formation plus two descriptors per intermediate merge.
/// Counting one record per OPF byte deliberately overestimates I and R.
pub fn storage_bound(entries: u32, opf_bytes: u32) -> Option<u64> {
    if entries == 0 || entries > MAX_MEMBERS { return None; }
    let e = u64::from(entries);
    let i = u64::from(opf_bytes.min(MAX_MEMBERS));
    let r = u64::from(opf_bytes.min(MAX_SPINE));
    let first = e.min(i).min(r);
    let initial = 24 * e + 24 * i + 12 * r;
    let joins = 16 * r + 8 * first + 12 * r;
    let intermediate = 2 * (24 * i + 12 * r + 16 * r);
    let m_runs = (24 * e).div_ceil(32760);
    let metadata_runs = 2 * ((24 * i + 12 * r).div_ceil(32744) + 2);
    let j1_runs = (16 * r).div_ceil(512);
    let f_runs = (8 * first).div_ceil(512);
    let j2_runs = (12 * r).div_ceil(6648);
    let headers = 24 * (m_runs + metadata_runs + j1_runs + f_runs + j2_runs + 2 * (metadata_runs + j1_runs));
    let payload = 384 + e * 32 + r * 16;
    let envelope = payload + payload.div_ceil(4096) * 4 + 64;
    Some(initial + joins + intermediate + headers + envelope)
}
