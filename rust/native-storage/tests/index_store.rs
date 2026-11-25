// SPDX-License-Identifier: MIT
//! Book index store over the real chapter cache, block envelope and Index
//! census, on the existing crashable FAT model.
extern crate alloc;
pub use newt_native_storage::*;
#[allow(dead_code)] // Reuse the complete existing FAT model.
#[path = "../../tests/sd_reading_state/model.rs"]
mod model;

use chapter_cache::{ChapterCacheState, ChapterCacheStorage};
use model::{Effect, Fault, Model, Op};
use newt_core::book_index::{self, Header, Identity, Member, Reader, SpineItem, WriteError, Writer, NONE, TITLE_BYTES};
use newt_core::phased_metadata::BookLanguage;
use newt_core::page_cache::CacheStorage;
use newt_core::page_cache_access::VerifiedAccess;
use newt_core::page_envelope::VerifiedStore;
use std::collections::BTreeMap;

const ROOT: &str = "/sd/Cache";
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BOOK: Identity = Identity { digest: [0xa5; 32], policy: 1 };

/// The FAT model plus what it lacks: file times for eviction order, a count
/// of open files for the card's small descriptor budget, and a power cut that
/// stops the card, so no cleanup runs between the cut and the next boot.
#[derive(Default)]
struct Card {
    model: Model,
    times: BTreeMap<String, i64>,
    open: u32,
    most_open: u32,
    creates: u32,
    cstr_opens: u32,
    writes: u32,
    armed: bool,
    off: bool,
}
impl Card {
    fn live<T>(&mut self, op: impl FnOnce(&mut Model) -> Result<T>) -> Result<T> {
        if self.off {
            return Err(Error::Unavailable);
        }
        let result = op(&mut self.model);
        self.off = self.armed && self.model.cut_after.is_none();
        result
    }
    /// Cut power `steps` card operations from now.
    fn cut_in(&mut self, steps: u32) {
        self.model.cut_after = Some(self.model.steps + steps);
        self.armed = true;
    }
    /// Boot again on what was durable at the cut.
    fn boot(&mut self) {
        (self.armed, self.off, self.open) = (false, false, 0);
        self.model.power();
    }
}
impl Native for Card {
    fn open_cstr(&mut self, path: &core::ffi::CStr, mode: Open) -> Result<i32> {
        self.cstr_opens += 1;
        self.open(path.to_str().map_err(|_| Error::Other)?, mode)
    }
    fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
        if matches!(mode, Open::CreateExclusive) { self.creates += 1; }
        let fd = self.live(|m| m.open(path, mode))?;
        self.open += 1;
        self.most_open = self.most_open.max(self.open);
        Ok(fd)
    }
    fn close(&mut self, fd: i32) -> Result<()> {
        self.live(|m| m.close(fd))?;
        self.open -= 1;
        Ok(())
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> { self.live(|m| m.read(fd, out)) }
    fn write(&mut self, fd: i32, input: &[u8]) -> Result<usize> { self.writes += 1; self.live(|m| m.write(fd, input)) }
    fn seek(&mut self, fd: i32, offset: u32) -> Result<u64> { self.live(|m| m.seek(fd, offset)) }
    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()> { self.live(|m| m.truncate(fd, bytes)) }
    fn sync(&mut self, fd: i32) -> Result<()> { self.live(|m| m.sync(fd)) }
    fn fstat(&mut self, fd: i32) -> Result<Stat> { self.live(|m| m.fstat(fd)) }
    fn stat(&mut self, path: &str, no_follow: bool) -> Result<Stat> {
        let mut stat = self.live(|m| m.stat(path, no_follow))?;
        stat.seconds = self.times.get(path).copied().unwrap_or_default();
        Ok(stat)
    }
    fn mkdir(&mut self, path: &str) -> Result<()> { self.live(|m| m.mkdir(path)) }
    fn unlink(&mut self, path: &str) -> Result<()> { self.live(|m| m.unlink(path)) }
    fn rename(&mut self, from: &str, to: &str) -> Result<()> { self.live(|m| m.rename(from, to)) }
    fn next_nonce(&mut self) -> u32 { self.model.next_nonce() }
}
impl Directory for Card {
    type Listing = u32;
    fn open_dir(&mut self, path: &str) -> Result<u32> { self.live(|m| m.open_dir(path)) }
    fn read_dir(&mut self, listing: &mut u32, name: &mut [u8]) -> Result<Option<usize>> {
        self.live(|m| m.read_dir(listing, name))
    }
    fn close_dir(&mut self, listing: u32) -> Result<()> { self.live(|m| m.close_dir(listing)) }
}
impl page_entries::PageEntries for Card {
    fn walk_entries(&mut self, folder: &core::ffi::CStr, sized: bool,
        visit: &mut dyn FnMut(&mut Self, page_entries::PageEntry<'_>) -> bool,
    ) -> bool { page_entries::walk_directory(self, folder, sized, visit) }
}
impl Card {
    fn new() -> Self {
        let mut card = Self::default();
        card.model.dirs.extend(["/sd".into(), ROOT.into()]);
        card
    }
    /// Files in Cache/Index, by leaf name.
    fn indexes(&self) -> Vec<String> {
        let folder = format!("{ROOT}/Index/");
        self.model.files.keys().filter_map(|p| p.strip_prefix(&folder)).map(String::from).collect()
    }
    fn seed(&mut self, leaf: &str, bytes: usize, time: i64) {
        let path = format!("{ROOT}/Index/{leaf}");
        self.model.dirs.insert(format!("{ROOT}/Index"));
        self.model.files.insert(path.clone(), model::File { durable: vec![0; bytes], readable: vec![0; bytes] });
        self.times.insert(path, time);
    }
}

struct Never;
impl Authority for Never {
    fn cancelled(&self) -> bool { false }
}
fn now() -> u32 { 0 }

fn leaf(digest: &[u8; 32]) -> String {
    let mut name = [0; 69];
    book_index::name(digest, &mut name);
    format!("{KEY}-{}", std::str::from_utf8(&name).unwrap())
}

/// One cold owner per call, closed afterwards like the reader's.
fn with_store<R>(card: &mut Card, work: impl FnOnce(&mut ChapterCacheStorage<'_, Card>) -> R) -> R {
    let mut state = ChapterCacheState::new();
    let route = RouteState::new(ROOT, KEY);
    let route = route.lend(&Never, now);
    let mut store = ChapterCacheStorage::lend(&mut state, card, &route).unwrap();
    let result = work(&mut store);
    store.access().close();
    result
}

/// 200 members span two 4 KiB blocks, so a later block is read on demand.
fn members() -> Vec<Member> {
    (0..200u32).map(|i| Member {
        hash: u64::from(i + 1) * 0x9e37_79b9,
        local_offset: 64 * i,
        compressed: 50,
        uncompressed: 100,
        method: 8,
        first_spine: if i < 3 { i } else { NONE },
        ..Member::default()
    }).collect()
}
const SPINE: [SpineItem; 3] = [
    SpineItem { member: 0, before: 0 },
    SpineItem { member: 1, before: 100 },
    SpineItem { member: 2, before: 200 },
];
fn header(identity: Identity) -> Header {
    let mut title = [0; TITLE_BYTES];
    title[..6].copy_from_slice(b"A Book");
    Header {
        identity, members: 200, spine: 3, spine_bytes: 300, nav: 3, ncx: NONE, cover: NONE,
        language: BookLanguage::Default, title,
    }
}

fn write(store: &mut ChapterCacheStorage<'_, Card>, identity: Identity) -> core::result::Result<(), WriteError> {
    let mut access = store.access();
    let mut buffer = [0; 4096];
    let mut writer = Writer::begin(&mut access, header(identity), &mut buffer)?;
    for member in members() {
        writer = writer.member(&mut access, member)?;
    }
    for item in SPINE {
        writer = writer.spine(&mut access, item)?;
    }
    writer.finish(&mut access, &mut || true)
}

/// The reader's verified path: attach the envelope, then read records
/// through it. None when the envelope or the header is refused.
fn read<R>(store: &mut ChapterCacheStorage<'_, Card>, identity: &Identity,
    work: impl FnOnce(&mut dyn CacheStorage, &Reader) -> R,
) -> Option<R> {
    let mut name = [0; 69];
    book_index::name(&identity.digest, &mut name);
    let (mut directory, mut window) = ([0; 4160], [0; 4096]);
    let mut access = store.access();
    let mut verified = VerifiedStore::new(Some(&name), &mut directory, &mut window);
    if !access.begin_page_read(false, Some(&mut verified.query)) {
        return None;
    }
    let mut result = None;
    if verified.attach(&mut access) {
        let mut view = VerifiedAccess { store: &mut verified, source: &mut access };
        if let Some(reader) = Reader::open(&mut view, identity) {
            result = Some(work(&mut view, &reader));
        }
    }
    verified.query.finish();
    assert!(access.finish_cache_read());
    result
}

/// Every record in order, plus lookups at both ends and across a block
/// boundary. One read scope allows 128 raw block reads, so a scope holds a
/// few searches, not one per member.
fn complete(s: &mut dyn CacheStorage, reader: &Reader) -> bool {
    let members = members();
    *reader.header() == header(reader.header().identity)
        && members.iter().enumerate().all(|(i, m)| reader.member(s, i as u32) == Some(*m))
        && SPINE.iter().enumerate().all(|(i, item)| reader.spine(s, i as u32) == Some(*item))
        && [0, 123, 124, 199].iter().all(|&i| reader.find(s, members[i].hash) == Ok(Some((i as u32, members[i]))))
        && reader.find(s, members[123].hash + 1) == Ok(None)
}

#[test]
fn an_index_round_trips_with_one_open_file() {
    let mut card = Card::new();
    with_store(&mut card, |store| write(store, BOOK)).unwrap();
    assert_eq!(card.indexes(), [leaf(&BOOK.digest)]);
    assert_eq!(with_store(&mut card, |store| read(store, &BOOK, complete)), Some(true));
    assert_eq!((card.open, card.most_open), (0, 1));
}

#[test]
fn a_damaged_stale_or_foreign_index_is_refused() {
    let mut card = Card::new();
    with_store(&mut card, |store| write(store, BOOK)).unwrap();
    let path = format!("{ROOT}/Index/{}", leaf(&BOOK.digest));
    let good = card.model.files[&path].clone();

    // Another admission policy: same name and blocks, refused by the header.
    let stale = Identity { policy: 2, ..BOOK };
    assert_eq!(with_store(&mut card, |store| read(store, &stale, |_, _| ())), None);

    // Another book's bytes under this name fail the route-seeded block checks.
    let other = Identity { digest: [0x5a; 32], ..BOOK };
    let foreign = format!("{ROOT}/Index/{}", leaf(&other.digest));
    card.model.files.insert(foreign, good.clone());
    assert_eq!(with_store(&mut card, |store| read(store, &other, |_, _| ())), None);

    // A flipped byte in the header block refuses the open; one in member
    // 152, in the second block, refuses only records read from that block.
    for (at, opens) in [(60, false), (384 + 152 * 32, true)] {
        let mut bad = good.clone();
        bad.durable[at] ^= 1;
        bad.readable[at] ^= 1;
        card.model.files.insert(path.clone(), bad);
        let found = with_store(&mut card, |store| read(store, &BOOK, |s, reader| {
            (reader.member(s, 0).is_some(), reader.member(s, 152))
        }));
        assert_eq!(found, opens.then_some((true, None)), "byte {at}");
    }
}

#[test]
fn a_full_card_or_power_cut_never_publishes_a_partial_index() {
    let mut card = Card::new();
    card.model.faults.push(Fault::new(Op::Write, 3, 1, Effect::Errno("ENOSPC"), 0));
    assert_eq!(with_store(&mut card, |store| write(store, BOOK)), Err(WriteError::Storage));
    assert_eq!(card.indexes(), Vec::<String>::new());

    // A first build, then a rebuild over it (a new admission policy), cut
    // at every card operation. The card stops at the cut and boots on what
    // was durable then.
    let rebuilt = Identity { policy: 2, ..BOOK };
    let steps = |rebuild: bool| {
        let mut card = Card::new();
        if rebuild {
            with_store(&mut card, |store| write(store, BOOK)).unwrap();
        }
        let start = card.model.steps;
        with_store(&mut card, |store| write(store, rebuilt)).unwrap();
        card.model.steps - start
    };
    for rebuild in [false, true] {
        for cut in 1..=steps(rebuild) {
            let mut card = Card::new();
            if rebuild {
                with_store(&mut card, |store| write(store, BOOK)).unwrap();
            }
            card.cut_in(cut);
            let written = with_store(&mut card, |store| write(store, rebuilt));
            card.boot();
            let old = with_store(&mut card, |store| read(store, &BOOK, complete));
            let new = with_store(&mut card, |store| read(store, &rebuilt, complete));
            // Each index is whole or absent; a rebuild never loses the old one
            // before the new one is published.
            assert!(matches!(old, None | Some(true)) && matches!(new, None | Some(true)), "cut {cut}");
            assert!(written.is_err() || new == Some(true), "cut {cut}");
            assert!(!rebuild || old.is_some() || new.is_some(), "cut {cut}");
            // The next census sweeps any leftover temporary, and the build
            // runs again.
            with_store(&mut card, |store| store.index_census(&Never, None, None, None));
            assert!(card.indexes().iter().all(|leaf| !leaf.ends_with(".tmp")), "cut {cut}: {:?}", card.indexes());
            with_store(&mut card, |store| write(store, rebuilt)).unwrap();
            assert_eq!(with_store(&mut card, |store| read(store, &rebuilt, complete)), Some(true), "cut {cut}");
        }
    }
}

#[test]
fn the_census_evicts_the_oldest_unpinned_index_and_keeps_the_open_one() {
    const MIB: usize = 1024 * 1024;
    let mut card = Card::new();
    let (pinned, orphan, live) = (leaf(&[1; 32]), leaf(&[2; 32]), leaf(&[3; 32]));
    // 14 MiB leaves less than one largest index free under the 16 MiB quota.
    card.seed(&pinned, 6 * MIB, 1);
    card.seed(&orphan, 4 * MIB, 2);
    card.seed(&live, 4 * MIB, 3);
    card.seed(&format!("{}.0000002a.tmp", leaf(&[4; 32])), 100, 9);
    let grant = with_store(&mut card, |store| {
        store.index_census(&Never, None, None, Some(pinned.as_bytes()))
    });
    assert_eq!(grant.outcome, MissingProbe::Ready);
    assert_eq!(card.indexes(), [pinned.clone(), live.clone()]);

    // The over-quota open still writes and reads its own index.
    with_store(&mut card, |store| write(store, BOOK)).unwrap();
    assert_eq!(with_store(&mut card, |store| read(store, &BOOK, complete)), Some(true));
    assert_eq!(card.indexes(), [pinned, live, leaf(&BOOK.digest)]);
}

#[test]
fn a_rebuilt_index_and_its_backup_stay_evictable() {
    const MIB: usize = 1024 * 1024;
    let mut card = Card::new();
    // A rebuild keeps the old copy as `.bak`, both written at time 0.
    with_store(&mut card, |store| write(store, BOOK)).unwrap();
    with_store(&mut card, |store| write(store, Identity { policy: 2, ..BOOK })).unwrap();
    let book = leaf(&BOOK.digest);
    assert_eq!(card.indexes(), [book.clone(), format!("{book}.bak")]);

    // The open book also has a pair; with a newer orphan, the folder is
    // crowded. The oldest unpinned files go first, backups included.
    let pinned = leaf(&[1; 32]);
    card.seed(&pinned, 6 * MIB, 5);
    card.seed(&format!("{pinned}.bak"), 6 * MIB, 1);
    card.seed(&leaf(&[2; 32]), 2 * MIB, 3);
    let grant = with_store(&mut card, |store| {
        store.index_census(&Never, None, None, Some(pinned.as_bytes()))
    });
    assert_eq!(grant.outcome, MissingProbe::Ready);
    assert_eq!(card.indexes(), [pinned.clone(), format!("{pinned}.bak")]);
}

use newt_core as core_api;
#[path = "../../core/tests/book_index_build/support.rs"]
mod build_support;
use chapter_cache::{IndexBuildStore, IndexWorkState};

fn spilling_build(card: &mut Card) -> core::result::Result<(), newt_core::book_index_build::Error> {
    use newt_core::book_index_build::Error as BuildError;
    let mut work = IndexWorkState::new();
    let mut buffer = [0; 448];
    let mut join = build_support::JoinBacking::new();
    let mut state = join.state();
    // Sweep before creating a live work file, never between these two loans.
    with_store(card, |store| store.index_census(&Never, None, None, None));
    let source = card.open("/sd/source", Open::SourceReadOnly).map_err(|_| BuildError::Write(WriteError::Storage))?;
    let prepared = with_store(card, |store| {
        let mut store = IndexBuildStore::lend(store, &mut work, &BOOK.digest)
            .ok_or(BuildError::Write(WriteError::Storage))?;
        build_support::prepare(&mut store, &mut buffer, 64, 8, 8, 8, &mut state)
    });
    let closed = card.close(source).is_ok();
    with_store(card, |store| {
        let mut store = IndexBuildStore::lend(store, &mut work, &BOOK.digest)
            .ok_or(BuildError::Write(WriteError::Storage))?;
        if !closed {
            newt_core::book_index_build::WorkFile::remove_work(&mut store);
            return Err(BuildError::Write(WriteError::Storage));
        }
        prepared?.publish(&mut store, &mut state, &mut |_| true, &mut || true).map(|_| ())
    })
}
fn built_index_complete(store: &mut dyn CacheStorage, reader: &Reader) -> bool {
    let h = reader.header();
    if (h.members, h.spine, h.spine_bytes) != (8, 8, 80) { return false; }
    (0..8).all(|i| {
        let Some(s) = reader.spine(store, i) else { return false; };
        let Some(m) = reader.member(store, s.member) else { return false; };
        s.before == u64::from(i) * 10 && m.local_offset == i * 64 && m.first_spine == i
    })
}
fn build_card() -> Card {
    let mut card = Card::new();
    for path in ["/sd/source", "/sd/receipt"] {
        card.model.files.insert(path.into(), model::File { durable: vec![0], readable: vec![0] });
    }
    card
}
#[test]
fn spilling_build_cuts_every_card_step_and_uses_three_files() {
    let mut good = build_card();
    let receipt = good.open("/sd/receipt", Open::SourceReadOnly).unwrap();
    let start = good.model.steps;
    spilling_build(&mut good).unwrap();
    let steps = good.model.steps - start;
    good.close(receipt).unwrap();
    assert_eq!(good.most_open, 3);
    assert_eq!(with_store(&mut good, |s| read(s, &BOOK, built_index_complete)), Some(true));
    for cut in 1..=steps {
        let mut card = build_card();
        card.open("/sd/receipt", Open::SourceReadOnly).unwrap();
        card.cut_in(cut);
        let _ = spilling_build(&mut card);
        assert!(card.most_open <= 3, "cut {cut}");
        card.boot();
        let found = with_store(&mut card, |s| read(s, &BOOK, built_index_complete));
        assert!(matches!(found, None | Some(true)), "cut {cut}: partial index");
        with_store(&mut card, |s| s.index_census(&Never, None, None, None));
        assert!(card.indexes().iter().all(|n| !n.ends_with(".tmp")), "cut {cut}: leftover");
        spilling_build(&mut card).unwrap();
        assert_eq!(with_store(&mut card, |s| read(s, &BOOK, built_index_complete)), Some(true), "cut {cut}: rebuild");
    }
    let work_bytes = core::mem::size_of::<IndexWorkState>();
    let view_bytes = core::mem::size_of::<IndexBuildStore<'_, '_, Card>>();
    println!("SPILLING CARD cuts={steps} most_open={} work_state={work_bytes} view={view_bytes}", good.most_open);
}

#[test]
fn resident_publication_keeps_source_and_receipt_open_with_three_files() {
    let mut card = build_card();
    let source = card.open("/sd/source", Open::SourceReadOnly).unwrap();
    let receipt = card.open("/sd/receipt", Open::SourceReadOnly).unwrap();
    let mut work = IndexWorkState::new();
    let mut buffer = [0; 32768];
    let mut join = build_support::JoinBacking::new();
    let mut state = join.state();
    with_store(&mut card, |cache| {
        let mut store = IndexBuildStore::lend(cache, &mut work, &BOOK.digest).unwrap();
        let prepared = build_support::prepare(&mut store, &mut buffer, 32768, 8, 8, 8, &mut state).unwrap();
        assert!(!store.spilled());
        assert!(store.release_pages());
        let report = prepared.publish(&mut store, &mut state, &mut |_| true, &mut || true).unwrap();
        assert!(report.work_removed);
    });
    let writes = (card.creates, card.writes);
    assert_eq!(writes.0, 1, "one native index temporary, no work file");
    assert!(writes.1 > 0);
    assert_eq!(with_store(&mut card, |s| read(s, &BOOK, built_index_complete)), Some(true));
    assert_eq!(with_store(&mut card, |s| read(s, &BOOK, built_index_complete)), Some(true));
    assert_eq!((card.creates, card.writes), writes, "repeat hits do not write");
    assert_eq!((card.open, card.most_open), (2, 3));
    card.close(source).unwrap();
    card.close(receipt).unwrap();
}


#[test]
fn published_build_requires_the_verified_read_scope() {
    let mut card = build_card();
    spilling_build(&mut card).unwrap();
    with_store(&mut card, |cache| {
        let mut work = IndexWorkState::new();
        let mut store = IndexBuildStore::lend(cache, &mut work, &BOOK.digest).unwrap();
        let mut name = [0; 69];
        book_index::name(&BOOK.digest, &mut name);
        assert_eq!(CacheStorage::read_at(&mut store, &name, 0, &mut [0; 384]), -1);
        assert!(Reader::open(&mut store, &BOOK).is_none());
        assert!(store.release_pages());
    });
    assert_eq!(with_store(&mut card, |s| read(s, &BOOK, built_index_complete)), Some(true));
}

#[test]
fn work_lend_creates_parent_and_spill_borrows_terminated_path() {
    use newt_core::book_index_build::WorkFile;
    let mut card = Card::new();
    let mut work = IndexWorkState::new();
    with_store(&mut card, |cache| {
        let mut store = IndexBuildStore::lend(cache, &mut work, &BOOK.digest).unwrap();
        assert!(!store.spilled());
        assert!(store.remove_work());
    });
    assert!(card.model.dirs.contains(&format!("{ROOT}/Index")));
    assert_eq!((card.creates, card.cstr_opens), (0, 0));
    with_store(&mut card, |cache| {
        let mut store = IndexBuildStore::lend(cache, &mut work, &BOOK.digest).unwrap();
        assert!(store.append(b"record"));
        assert!(store.spilled());
        assert!(store.append(b"next"));
        assert!(store.remove_work());
    });
    assert_eq!((card.creates, card.cstr_opens), (1, 1));
    assert!(card.indexes().is_empty());
}
