// SPDX-License-Identifier: MIT
//! Cold resume with an old Pages envelope over the firmware's real cache and State ports.
use super::*;
use newt_core::{
    durable::{Commit, Load}, external_progress_store::ExternalProgressStore, page_envelope,
    producer_visit::MissingProbe, progress::Locator, zip_container::Status,
};
use newt_native_io::UiIo;
use newt_native_storage::{chapter_cache::{ChapterCacheState, ChapterCacheStorage},
    reading_state::{ReadingStateRouter, StateFacts}, Route, RouteState};
use newt_phased_reader::{CacheRequest, OperationPhased, PhasedChapterCache,
    imported_revision::session::{Build, Cache}};
use newt_phased_sink::{FRAME_BYTES, Target};
use reading_state_medium::MountedStateMedium;
use std::collections::BTreeMap;
#[path = "../legacy_nvs/flash.rs"]
mod flash;

type FixtureState<'s, 'io> = ReadingStateRouter<'s, nvs::NvsBlobBackend<'s, flash::Flash>,
    card::Port<'io>, MountedStateMedium<'s, 's, 's, 'io, card::Port<'io>>>;

fn with_state<R>(card: &mut card::Card, io: &UiIo, digest: &[u8; 32],
    work: impl FnOnce(&mut ExternalProgressStore<'_, FixtureState<'_, '_>>) -> R,
) -> R {
    let mut flash = flash::Flash::default();
    let mut state = StateFacts::default();
    let mut medium = MountedStateMedium::<card::Port<'_>>::new(io, None);
    let mut native = card.port(io);
    let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash),
        &mut state, &mut medium, &mut native);
    work(&mut ExternalProgressStore::new(&mut store, Some(digest)))
}

fn with_pages(card: &mut card::Card, io: &UiIo, route: &Route<'_>,
    work: impl FnOnce(&mut reader_pages::ReaderPages<'_, card::Port<'_>>),
) {
    let mut native = card.port(io);
    let mut state = ChapterCacheState::new();
    let mut pages = reader_pages::ReaderPages(ChapterCacheStorage::lend(&mut state, &mut native, route).unwrap());
    work(&mut pages);
    pages.0.access().close();
}

fn card_files(card: &card::Card) -> BTreeMap<String, Vec<u8>> {
    card.0.borrow().model.files.iter()
        .map(|(name, file)| (name.clone(), file.durable.clone())).collect()
}

fn seed_old_pages(card: &card::Card, files: &BTreeMap<String, Vec<u8>>, version: u32) -> (String, Vec<u8>) {
    let (path, current) = files.iter().find(|(path, _)| path.ends_with(".fibp")).unwrap();
    assert!(path.contains("/Pages/"));
    let mut old = current.clone();
    let manifest = old.len() - page_envelope::MANIFEST_BYTES as usize;
    assert_eq!(&old[manifest..manifest + 4], b"NTBE");
    // Valid older envelope, including the checksum. Keep the exact filename.
    old[manifest + 4..manifest + 8].copy_from_slice(&version.to_le_bytes());
    let checksum = page_envelope::crc(u32::MAX, &old[manifest..manifest + 60]) ^ u32::MAX;
    old[manifest + 60..].copy_from_slice(&checksum.to_le_bytes());
    card.seed(path, old.clone());
    (format!("{path}.bak"), old)
}

pub fn resume(bytes: Vec<u8>) {
    for old_version in [1u32, 2] {
        resume_from(bytes.clone(), old_version);
    }
    println!("PASS old Pages data misses, saved content resumes, and only Pages rebuilds");
}

fn resume_from(bytes: Vec<u8>, old_version: u32) {
    let mut tokens = [0u8; 2];
    // SAFETY: the existing host Board fake treats these as disjoint tokens.
    let (mut board, pending, _) = unsafe { newt_c3_board::Board::acquire_x3_optional_sd(
        (&mut tokens[0] as *mut u8).cast(), (&mut tokens[1] as *mut u8).cast()) }.unwrap();
    drop(pending);
    board.with_reader_io(|panel, io| {
        let mut source = Source(bytes);
        let mut digest = [0; 32];
        assert!(hash(&source.0, &mut digest));
        let book = u64::from_le_bytes(digest[..8].try_into().unwrap());
        let mut card = card::Card::default();
        let mut bank = [0; BANK_BYTES];
        let mut storage = Storage::new(&mut bank);
        let mut parser = newt_target_expat::Expat::default();
        let mut breaker = linebreak::NativeBreak;
        let route = RouteState::new(reader_book::CACHE_ROOT,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
        let route = route.lend(&Never, now);
        let settings = ViewSettings { orientation: newt_core::preferences::Orientation::Portrait,
            ..ViewSettings::default() };
        let mut selections = [Selection::default(), Selection::default()];
        let mut runs = [Run::default(); RUNS];
        let mut text = [0; TEXT];
        let mut plan = Plan::new(&mut runs, &mut text);
        let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), false);
        let mut frame = [0; FRAME_BYTES];

        assert_eq!(reader.open_phased(&mut source, &mut storage, panel, &mut parser), Status::Ok);
        assert_eq!(reader.capture_phased(&mut source, 0, 2, Target::Page, &mut storage,
            panel, &mut parser, &mut breaker), Status::Ok);
        assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
        let page = reader.info();
        let expected_text = reader.page_plan().unwrap().text().to_vec();
        let mut saved = Progress { book, locator: Locator { spine: 0, character: page.char_start + 1 },
            ..Progress::default() };

        assert_eq!(with_state(&mut card, io, &digest, |state|
            state.save(&mut saved, None, None, false, None)), Commit::Saved);
        with_pages(&mut card, io, &route, |pages| {
            let mut cache = PhasedChapterCache { store: pages, hash: &mut Hash, settings };
            assert_eq!(reader.open_phased(&mut source, &mut storage, panel, &mut parser), Status::Ok);
            let mut operation = OperationPhased::new(&mut reader, &mut storage, panel, &mut parser, &mut breaker);
            assert_eq!(cache.build(&mut source, &digest, 0, &mut operation), Build::Ready);
        });
        let mut files = card_files(&card);
        assert!(files.keys().any(|path| path.contains("/State/")));
        let (backup, legacy) = seed_old_pages(&card, &files, old_version);
        assert_ne!(card_files(&card), files);

        // Recreate the card owners from durable bytes only.
        let mut card = card.cold_bytes();
        let mut restored = Progress::default();
        assert_eq!(with_state(&mut card, io, &digest, |state| state.load(&mut restored)), Load::Loaded(()));
        assert_eq!(restored.locator, saved.locator);
        with_pages(&mut card, io, &route, |pages| {
            use newt_reader_ui::{PhysicalOwner, RestoreFailure};
            let mut owner = ExternalOwner::default();
            let mut authorize = |raw: &mut dyn BookSource, expected: &[u8; 32], _: &mut [u8], allowed: &dyn Fn() -> bool| {
                let mut bytes = vec![0; raw.size() as usize];
                let mut actual = [0; 32];
                allowed() && raw.read_at(0, &mut bytes) == bytes.len() as i32
                    && hash(&bytes, &mut actual) && &actual == expected
            };
            let mut source = StrictSource::new(&mut source, &digest).with_reopen_authorizer(&mut authorize, &|| true);
            let mut ports = PhysicalOwner { storage: &mut storage, panel, backend: &mut parser, breaker: &mut breaker,
                cache: PhasedChapterCache { store: pages, hash: &mut Hash, settings },
                open: PhysicalOpen::Import, cache_only: false };
            assert!(!owner.open_physical(&mut reader, &mut source, book, restored,
                RestorePolicy::ExactPage, now(), &mut ports));
            assert_eq!(owner.restore_failure, RestoreFailure::InvalidCheckpoint);
            let cold = reader_restore_policy::boot_restore_policy(
                Some(reader_restore_policy::DurableCheckpoint::PageStart), None);
            for policy in [RestorePolicy::Validated, cold] {
                assert!(owner.open_physical(&mut reader, &mut source, book, restored, policy, now(), &mut ports));
                assert_eq!(reader.page_plan().unwrap().text(), expected_text);
                assert_eq!(reader.info().char_start, page.char_start);
            }
            let outside = Progress { locator: Locator { character: u32::MAX, ..restored.locator }, ..restored };
            assert!(!owner.open_physical(&mut reader, &mut source, book, outside,
                cold, now(), &mut ports));
            assert_eq!(owner.restore_failure, RestoreFailure::InvalidCheckpoint);
        });
        with_pages(&mut card, io, &route, |pages| {
            let mut cache = PhasedChapterCache { store: pages, hash: &mut Hash, settings };
            assert_eq!(reader.open_phased(&mut source, &mut storage, panel, &mut parser), Status::Ok);
            let mut op = OperationPhased::new(&mut reader, &mut storage, panel, &mut parser, &mut breaker);
            let request = CacheRequest { spine: restored.locator.spine, target: restored.locator.character,
                anchor: true, staged: false, build_on_miss: false, cache_only: false };
            let mut parsed = false;
            assert_eq!(cache.producer_stored(&mut source, &digest, 0, None, &|| false, &mut parsed, &mut op),
                MissingProbe::Ready, "old Pages version must be missing for the producer");
            let started = std::time::Instant::now();
            let shown = cache.capture(&mut source, &digest, request, &mut op);
            let miss = started.elapsed();
            assert_eq!(shown.status, Status::Ok);
            assert!(!shown.hit, "old Pages envelope was accepted");
            assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
            assert_eq!(reader.info().char_start, page.char_start);
            assert_eq!(reader.page_plan().unwrap().text(), expected_text);
            assert_eq!(reader.open_phased(&mut source, &mut storage, panel, &mut parser), Status::Ok);
            let mut op = OperationPhased::new(&mut reader, &mut storage, panel, &mut parser, &mut breaker);
            let started = std::time::Instant::now();
            assert_eq!(cache.build(&mut source, &digest, 0, &mut op), Build::Ready);
            let rebuild = started.elapsed();
            assert_eq!(cache.producer_stored(&mut source, &digest, 0, None, &|| false, &mut parsed, &mut op),
                MissingProbe::Complete, "new Pages version is ready for the producer");
            let started = std::time::Instant::now();
            assert!(cache.capture(&mut source, &digest, request, &mut op).hit);
            println!("host layout capture: miss={}us hit={}us rebuild={}us",
                miss.as_micros(), started.elapsed().as_micros(), rebuild.as_micros());
        });
        // The normal Pages transaction keeps the previous NTBE as a backup.
        assert!(files.insert(backup, legacy).is_none());
        let rebuilt = card_files(&card);
        let changed: Vec<_> = files.iter().filter(|(name, bytes)| rebuilt.get(*name) != Some(*bytes))
            .map(|(name, _)| name).collect();
        let added: Vec<_> = rebuilt.keys().filter(|name| !files.contains_key(*name)).collect();
        assert!(changed.is_empty() && added.is_empty(), "unexpected card changes: {changed:?}, added: {added:?}");
    }).unwrap();
}
