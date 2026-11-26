// SPDX-License-Identifier: MIT
//! Book index store over the real chapter cache, block envelope and Index
//! census, on the existing crashable FAT model.
extern crate alloc;
pub use newt_native_storage::*;
#[allow(dead_code)] // Reuse the complete existing FAT model.
#[path = "../../tests/sd_reading_state/model.rs"]
mod model;

use chapter_cache::{ChapterCacheState, ChapterCacheStorage};
use model::{Effect, Fault, Op};
use newt_core::book_index::{self, Header, Identity, Member, Reader, SpineItem, WriteError, Writer, NONE, TITLE_BYTES};
use newt_core::phased_metadata::BookLanguage;
use newt_core::page_cache::CacheStorage;
use newt_core::page_cache_access::VerifiedAccess;
use newt_core::page_envelope::VerifiedStore;

const ROOT: &str = "/sd/Cache";
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BOOK: Identity = Identity { digest: [0xa5; 32], policy: 1 };

#[path = "index_store/card.rs"]
mod card;
use card::Card;

struct Never;
impl Authority for Never {
    fn cancelled(&self) -> bool { false }
}
fn now() -> u32 { 0 }

fn leaf(digest: &[u8; 32]) -> String {
    book_index::FileName::new(digest,book_index::FileKind::Index).as_str().to_owned()
}

fn backup(name:&str)->String {
    book_index::FileName::parse(name.as_bytes()).unwrap().with_kind(book_index::FileKind::Backup).as_str().to_owned()
}
fn pair(name:&str)->Vec<String> {
    let mut names=vec![name.to_owned(),backup(name)];names.sort();names
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
        contents_spine: if i < 3 { i } else { NONE },
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
        stylesheets: false, navigation_declared: false, identity, members: 200, spine: 3, spine_bytes: 300, nav: 3, ncx: NONE, cover: NONE,
        language: BookLanguage::Default, title,
    }
}

fn write(store: &mut ChapterCacheStorage<'_, Card>, identity: Identity) -> core::result::Result<(), WriteError> {
    let mut access = store.access();
    let mut buffer = [0; 4096];
    write_records(&mut access, identity, &mut buffer)
}
fn write_records<S: CacheStorage + ?Sized>(store: &mut S, identity: Identity, buffer: &mut [u8]) -> core::result::Result<(), WriteError> {
    let mut writer = Writer::begin(store, header(identity), buffer)?;
    for member in members() {
        writer = writer.member(store, member)?;
    }
    for item in SPINE {
        writer = writer.spine(store, item)?;
    }
    writer.finish(store, &mut || true)
}

/// The reader's verified path: attach the envelope, then read records
/// through it. None when the envelope or the header is refused.
fn read<R>(store: &mut ChapterCacheStorage<'_, Card>, identity: &Identity,
    work: impl FnOnce(&mut dyn CacheStorage, &Reader) -> R,
) -> Option<R> {
    let name=*book_index::FileName::new(&identity.digest,book_index::FileKind::Index).as_bytes();
    let (mut directory, mut window) = ([0; 4160], [0; 4096]);
    let mut access = store.access();
    let mut verified = VerifiedStore::for_index(Some(&name),&identity.digest, &mut directory, &mut window);
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
fn v3_book_indexes_stay_admitted_while_v1_and_v2_are_refused() {
    use newt_core::page_envelope::{self, MANIFEST_BYTES};
    let mut card = Card::new();
    with_store(&mut card, |store| write(store, BOOK)).unwrap();
    let path = format!("{ROOT}/Index/{}", leaf(&BOOK.digest));
    let good = card.model.files[&path].clone();
    let restamp = |version: u32| {
        let mut stamped = good.clone();
        let at = stamped.durable.len() - MANIFEST_BYTES as usize;
        for buf in [&mut stamped.durable, &mut stamped.readable] {
            buf[at + 4..at + 8].copy_from_slice(&version.to_le_bytes());
            let checksum = page_envelope::crc(u32::MAX, &buf[at..at + 60]) ^ u32::MAX;
            buf[at + 60..].copy_from_slice(&checksum.to_le_bytes());
        }
        stamped
    };
    for rejected in [1u32, 2, 6] {
        card.model.files.insert(path.clone(), restamp(rejected));
        assert_eq!(with_store(&mut card, |store| read(store, &BOOK, complete)), None);
    }
    for admitted in [3u32, 4, 5] {
        card.model.files.insert(path.clone(), restamp(admitted));
        assert_eq!(with_store(&mut card, |store| read(store, &BOOK, complete)), Some(true));
    }
    // Backup fallback (.BAK) under an interrupted replace also stays admitted for v3/v4.
    let backup_path = format!("{ROOT}/Index/{}", backup(&leaf(&BOOK.digest)));
    card.model.files.remove(&path);
    for admitted in [3u32, 4] {
        card.model.files.insert(backup_path.clone(), restamp(admitted));
        assert_eq!(with_store(&mut card, |store| read(store, &BOOK, complete)), Some(true));
    }
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
    // A rebuild keeps the old copy as `.bak`, both written at time 1.
    with_store(&mut card, |store| write(store, BOOK)).unwrap();
    with_store(&mut card, |store| write(store, Identity { policy: 2, ..BOOK })).unwrap();
    let book = leaf(&BOOK.digest);
    assert_eq!(card.indexes(), pair(&book));

    // The open book also has a pair; with a newer orphan, the folder is
    // crowded. The oldest unpinned files go first, backups included: only the
    // open `.IDX` is pinned, not its own `.BAK`.
    let pinned = leaf(&[1; 32]);
    card.seed(&pinned, 6 * MIB, 5);
    card.seed(&backup(&pinned), 6 * MIB, 0);
    card.seed(&leaf(&[2; 32]), 8 * MIB, 3);
    let grant = with_store(&mut card, |store| {
        store.index_census(&Never, None, None, Some(pinned.as_bytes()))
    });
    assert_eq!(grant.outcome, MissingProbe::Ready);
    assert_eq!(card.indexes(), [pinned]);
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
    let source = card.open("/sd/source", Open::SourceReadOnly).map_err(|_| BuildError::Write(WriteError::Storage))?;
    let prepared = with_store(card, |store| {
        let mut store = IndexBuildStore::lend(store, &mut work, &BOOK.digest)
            .ok_or(BuildError::Write(WriteError::Storage))?;
        // The open path's reservation, before the work file exists.
        store.census(&|| false);
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
        s.before == u64::from(i) * 10 && m.local_offset == i * 64 && m.contents_spine == i
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

/// A Contents scan reads members in order, one chunk per checked scope. The
/// chunk must fit one scope's read allowance even at the largest index.
#[test]
fn one_read_scope_scans_a_contents_chunk_of_the_largest_index() {
    use book_index::{MAX_MEMBERS, SCAN_SCOPE_MEMBERS};
    let mut card = Card::new();
    let mut title = [0; TITLE_BYTES]; title[0] = b'T';
    let header = Header { stylesheets: false, navigation_declared: false, identity: BOOK, members: MAX_MEMBERS,
        spine: 1, spine_bytes: 1, nav: NONE, ncx: NONE, cover: NONE, language: BookLanguage::Default, title };
    with_store(&mut card, |store| {
        let mut access = store.access();
        let mut buffer = [0; 4096];
        let mut writer = Writer::begin(&mut access, header, &mut buffer)?;
        for i in 0..MAX_MEMBERS {
            writer = writer.member(&mut access, Member { hash: u64::from(i) + 1, ..Member::default() })?;
        }
        writer = writer.spine(&mut access, SpineItem { member: 0, before: 0 })?;
        writer.finish(&mut access, &mut || true)
    }).unwrap();
    let scan = |card: &mut Card, from: u32, count: u32| with_store(card, |store| read(store, &BOOK, |s, reader| {
        (from..from + count).all(|i| reader.member(s, i).is_some_and(|m| m.hash == u64::from(i) + 1))
    }));
    for from in [0, MAX_MEMBERS - SCAN_SCOPE_MEMBERS] {
        assert_eq!(scan(&mut card, from, SCAN_SCOPE_MEMBERS), Some(true), "chunk from {from}");
    }
    // The allowance is real: four chunks in one scope run out of reads.
    assert_eq!(scan(&mut card, 0, 4 * SCAN_SCOPE_MEMBERS), Some(false));
}

#[test]
fn a_spilled_build_at_a_full_folder_never_exceeds_the_cap() {
    let mut card = build_card();
    for i in 0..32u8 { card.seed(&leaf(&[i; 32]), 100, i64::from(i)); }
    spilling_build(&mut card).unwrap();
    // The `.WRK` and the index `.TMP` coexist, so the reservation needs two slots.
    assert_eq!(card.most_indexes, 32);
    assert_eq!(card.indexes().len(), 31);
    assert_eq!(with_store(&mut card, |s| read(s, &BOOK, built_index_complete)), Some(true));
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
        let name=*book_index::FileName::new(&BOOK.digest,book_index::FileKind::Index).as_bytes();
        assert_eq!(CacheStorage::read_at(&mut store, &name, 0, &mut [0; 384]), -1);
        assert!(Reader::open(&mut store, &BOOK).is_none());
        assert!(store.release_pages());
    });
    assert_eq!(with_store(&mut card, |s| read(s, &BOOK, built_index_complete)), Some(true));
}

#[test]
fn work_lend_creates_parent_and_spill_borrows_terminated_path() {
    use newt_core::book_index_build::WorkFile;
    let mut card = Card::default();
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

#[test]
fn index_publication_uses_the_fat_path_timestamp() {
    let mut card = Card::new();
    with_store(&mut card, |store| write(store, BOOK)).unwrap();
    let path = format!("{ROOT}/Index/{}", leaf(&BOOK.digest));
    let fd = card.open(&path, Open::ReadNonblock).unwrap();
    let descriptor = card.fstat(fd).unwrap();
    let named = card.stat(&path, false).unwrap();
    assert_eq!((descriptor.seconds, descriptor.nanos), (0, 0));
    assert_ne!((named.seconds, named.nanos), (0, 0));
    card.close(fd).unwrap();
    assert_eq!(with_store(&mut card, |store| read(store, &BOOK, complete)), Some(true));
    assert_eq!(card.open, 0);
}

#[test]
fn short_name_collision_refuses_the_other_digest_and_rebuilds() {
    let mut card=Card::new();let mut other=BOOK;other.digest[31]^=1;
    assert_eq!(leaf(&BOOK.digest),leaf(&other.digest));
    with_store(&mut card,|store|write(store,BOOK)).unwrap();
    assert_eq!(with_store(&mut card,|store|read(store,&other,complete)),None);
    with_store(&mut card,|store|write(store,other)).unwrap();
    assert_eq!(with_store(&mut card,|store|read(store,&other,complete)),Some(true));
    assert_eq!(with_store(&mut card,|store|read(store,&BOOK,complete)),None);
    with_store(&mut card,|store|write(store,BOOK)).unwrap();
    assert_eq!(with_store(&mut card,|store|read(store,&BOOK,complete)),Some(true));
    assert!(card.indexes().iter().all(|name|book_index::FileName::parse(name.as_bytes()).is_some()));
}
#[test]
fn legacy_long_names_are_ignored_and_swept_one_victim_bank_at_a_time() {
    let mut card=Card::new();
    with_store(&mut card,|store|write(store,BOOK)).unwrap();
    let good=card.model.files.remove(&format!("{ROOT}/Index/{}",leaf(&BOOK.digest))).unwrap();
    let mut old=[0;69];book_index::name(&BOOK.digest,&mut old);
    let legacy=format!("{KEY}-{}",core::str::from_utf8(&old).unwrap());
    card.model.files.insert(format!("{ROOT}/Index/{legacy}"),good);
    assert_eq!(with_store(&mut card,|store|read(store,&BOOK,complete)),None,"legacy names never supply an index hit");
    for i in 0..11 {card.seed(&format!("{KEY}-{i:064x}.fibx"),100,1);}
    let grant=with_store(&mut card,|store|store.index_census(&Never,None,None,None));
    assert_eq!(card.indexes().len(),4,"one bank of eight legacy victims per build");
    assert_eq!(grant.entries,20,"the census still counts all twelve entries");
    with_store(&mut card,|store|store.index_census(&Never,None,None,None));
    assert!(card.indexes().is_empty());
}
#[test]
fn cap_thirty_two_keeps_the_full_census_and_evicts_oldest_first() {
    use newt_core::cache_names::namespace::{INDEX_ENTRY_CAP,INDEX_ENTRY_GRANT};
    assert_eq!(INDEX_ENTRY_CAP,32);
    let mut card=Card::new();
    for i in 0..40u8 {card.seed(&leaf(&[i;32]),100,i64::from(i));}
    let grant=with_store(&mut card,|store|store.index_census(&Never,None,None,None));
    assert_eq!(grant.outcome,MissingProbe::Ready);
    assert_eq!(card.indexes().len(),usize::from(INDEX_ENTRY_GRANT)-1);
    assert_eq!(grant.entries,1);
    for i in 0..40u8 {assert_eq!(card.indexes().contains(&leaf(&[i;32])),i>=17);}
}
#[test]
fn stale_short_work_and_temp_slots_cannot_block_a_build() {
    use book_index::{FileName,FileKind};
    let mut card=build_card();
    for kind in [FileKind::Work,FileKind::Temporary] {
        card.seed(FileName::new(&BOOK.digest,kind).as_str(),100,1);
    }
    // Deliberately omit the census: each deterministic slot handles its own
    // interrupted predecessor before any exclusive create.
    with_store(&mut card,|cache| {
        let mut work=IndexWorkState::new();let mut buffer=[0;448];
        let mut backing=build_support::JoinBacking::new();let mut join=backing.state();
        let mut store=IndexBuildStore::lend(cache,&mut work,&BOOK.digest).unwrap();
        assert!(!store.spilled());
        let prepared=build_support::prepare(&mut store,&mut buffer,64,8,8,8,&mut join).unwrap();
        assert!(prepared.publish(&mut store,&mut join,&mut|_|true,&mut||true).unwrap().work_removed);
    });
    assert_eq!(card.indexes(),[leaf(&BOOK.digest)]);
    assert_eq!(with_store(&mut card,|store|read(store,&BOOK,built_index_complete)),Some(true));
}

#[test]
fn consumed_block_damage_can_revoke_both_index_copies_across_cold_boot() {
    use newt_core::page_cache::ReadScope;
    let mut card = Card::default();
    for _ in 0..2 { with_store(&mut card, |store| write(store, BOOK)).unwrap(); }
    let name = leaf(&BOOK.digest);
    let path = format!("{ROOT}/Index/{name}");
    let offset = book_index::HEADER_BYTES + 200 * book_index::MEMBER_BYTES;
    card.model.files.get_mut(&path).unwrap().readable[offset as usize] ^= 1;
    with_store(&mut card, |store| {
        let mut access = store.access();
        let (value, _) = access.query_index(name.as_bytes(), &BOOK.digest, false, &mut [0; 8256],
            &mut || true, |verified| {
                let index = Reader::open(verified, &BOOK).expect("header block is intact");
                index.spine(verified, 0)
            }).unwrap();
        assert!(value.is_none());
        assert!(ReadScope::invalidate_index(&mut access, &BOOK.digest));
    });
    card.boot();
    assert!(!card.model.files.contains_key(&path));
    assert!(!card.model.files.contains_key(&format!("{ROOT}/Index/{}", backup(&name))));
    with_store(&mut card, |store| assert!(read(store, &BOOK, |_, _| ()).is_none()));
}

/// Drop only the reservation hint to measure allocation independently of batching.
struct Unsized<'a, S: ?Sized>(&'a mut S);
impl<S: CacheStorage + ?Sized> CacheStorage for Unsized<'_, S> {
    fn remove(&mut self, name: &[u8]) -> bool { self.0.remove(name) }
    fn file_size(&mut self, name: &[u8]) -> i64 { self.0.file_size(name) }
    fn read_at(&mut self, name: &[u8], offset: u32, dst: &mut [u8]) -> i32 { self.0.read_at(name, offset, dst) }
    fn begin_write(&mut self, name: &[u8]) -> bool { self.0.begin_write(name) }
    fn write(&mut self, bytes: &[u8]) -> bool { self.0.write(bytes) }
    fn end_write(&mut self) -> bool { self.0.end_write() }
    fn end_write_checked(&mut self, live: &mut dyn FnMut() -> bool) -> bool { self.0.end_write_checked(live) }
    fn read_back_at(&mut self, offset: u32, dst: &mut [u8]) -> i32 { self.0.read_back_at(offset, dst) }
}

#[test]
fn publish_uses_larger_sequential_writes_without_changing_the_envelope() {
    let mut snapshots = Vec::new();
    let mut measurements = Vec::new();
    for mode in 0..3 {
        let mut card = Card::default();
        with_store(&mut card, |cache| {
            let mut work = chapter_cache::IndexWorkState::new();
            let mut index = chapter_cache::IndexBuildStore::lend(cache, &mut work, &BOOK.digest).unwrap();
            if mode == 2 { write_records(&mut index, BOOK, &mut [0; 4096]).unwrap(); }
            else if mode == 1 { write_records(&mut Unsized(&mut index), BOOK, &mut [0; 4096]).unwrap(); }
            else {
                drop(index);
                write_records(&mut cache.access(), BOOK, &mut [0; 4096]).unwrap();
            }
        });
        let seeks = card.model.trace.iter().filter(|line| line.starts_with("seek ")).count();
        let lookups = card.model.trace.iter().filter(|line| line.starts_with("stat ")).count();
        measurements.push((card.writes, seeks, lookups, card.allocations));
        snapshots.push(card.model.files[&format!("{ROOT}/Index/{}", leaf(&BOOK.digest))].durable.clone());
    }
    assert_eq!(snapshots[0], snapshots[1]);
    assert_eq!(snapshots[1], snapshots[2]);
    assert!(measurements[1].0 < measurements[0].0);
    assert!(measurements[1].1 < measurements[0].1);
    assert_eq!(measurements[1].2, measurements[0].2);
    assert_eq!(measurements[2].3, 1, "one allocation reserves the entire envelope");
    assert!(measurements[2].3 < measurements[1].3);
    assert_eq!(measurements[2].0, measurements[1].0);
    assert_eq!(measurements[2].1, measurements[1].1);
    assert_eq!(measurements[2].2, measurements[1].2);
    println!("INDEX IO legacy={:?} sequential={:?} reserved={:?} (writes,seeks,path_lookups,extent_growth_calls)", measurements[0], measurements[1], measurements[2]);
}

#[test]
fn reserved_index_retries_short_io_and_refuses_failed_allocation() {
    for (op, effect, copies, success) in [
        (Op::Write, Effect::Short(37), 1, true),
        (Op::Write, Effect::Errno("EINTR"), 1, true),
        (Op::Write, Effect::Errno("EINTR"), 4, false),
        (Op::Write, Effect::Short(0), 1, false),
        (Op::Truncate, Effect::Errno("ENOSPC"), 1, false),
    ] {
        let mut card = Card::default();
        card.model.faults.push(Fault::new(op, 1, copies, effect, 0));
        let result = with_store(&mut card, |cache| {
            let mut work = chapter_cache::IndexWorkState::new();
            let mut index = chapter_cache::IndexBuildStore::lend(cache, &mut work, &BOOK.digest).unwrap();
            write_records(&mut index, BOOK, &mut [0; 4096])
        });
        assert_eq!(result.is_ok(), success, "{op:?} {effect:?} x{copies}");
        card.boot();
        assert_eq!(with_store(&mut card, |store| read(store, &BOOK, complete)), success.then_some(true));
    }
}

#[path = "index_store/trim.rs"]
mod trim;
