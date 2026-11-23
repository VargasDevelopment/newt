// SPDX-License-Identifier: MIT
//! Cold resume with an old Pages envelope over the firmware's real cache and State ports.
use super::*;
use newt_core::{
    durable::{Commit, Load}, external_progress_store::ExternalProgressStore, page_envelope,
    progress::Locator, zip_container::Status,
};
use newt_native_storage::{chapter_cache::{ChapterCacheState, ChapterCacheStorage},
    reading_state::{ReadingStateRouter, StateFacts}, RouteState};
use newt_phased_reader::{CacheRequest, OperationPhased, PhasedChapterCache,
    imported_revision::session::{Build, Cache}};
use newt_phased_sink::{FRAME_BYTES, Target};
use reading_state_medium::MountedStateMedium;
#[path = "../legacy_nvs/flash.rs"]
mod flash;

pub fn resume(bytes: Vec<u8>) {
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
        let mut flash = flash::Flash::default();
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

        {
            let mut state = StateFacts::default();
            let mut medium = MountedStateMedium::<card::Port<'_>>::new(io, None);
            let mut native = card.port(io);
            let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash),
                &mut state, &mut medium, &mut native);
            assert_eq!(ExternalProgressStore::new(&mut store, Some(&digest))
                .save(&mut saved, None, None, false, None), Commit::Saved);
        }
        {
            let mut native = card.port(io);
            let mut state = ChapterCacheState::new();
            let mut pages = reader_pages::ReaderPages(ChapterCacheStorage::lend(&mut state, &mut native, &route).unwrap());
            let mut cache = PhasedChapterCache { store: &mut pages, hash: &mut Hash, settings };
            assert_eq!(reader.open_phased(&mut source, &mut storage, panel, &mut parser), Status::Ok);
            let mut operation = OperationPhased::new(&mut reader, &mut storage, panel, &mut parser, &mut breaker);
            assert_eq!(cache.build(&mut source, &digest, 0, &mut operation), Build::Ready);
            pages.0.access().close();
        }
        let (path, current) = {
            let state = card.0.borrow();
            let (path, file) = state.model.files.iter().find(|(path, _)| path.ends_with(".fibp")).unwrap();
            (path.clone(), file.durable.clone())
        };
        let mut old = current.clone();
        let manifest = old.len() - page_envelope::MANIFEST_BYTES as usize;
        // Valid v1 envelope, including the checksum. Keep the exact filename.
        old[manifest+4..manifest+8].copy_from_slice(&1u32.to_le_bytes());
        let checksum = page_envelope::crc(u32::MAX, &old[manifest..manifest+60]) ^ u32::MAX;
        old[manifest+60..].copy_from_slice(&checksum.to_le_bytes());
        card.seed(&path, old);
        card.seed("/newt-sd/Newt/Covers/generated.nlc", b"existing cover".to_vec());
        card.seed("/newt-sd/Newt/Cache/Glyphs/generated", b"existing glyph cache".to_vec());
        let before = card.0.borrow();
        let other: Vec<_> = before.model.files.iter().filter(|(name, _)| *name != &path)
            .map(|(name, file)| (name.clone(), file.durable.clone())).collect();
        drop(before);

        // Recreate the card owners from durable bytes only.
        let mut card = card.cold_bytes();
        let mut restored = Progress::default();
        {
            let mut flash = flash::Flash::default();
            let mut state = StateFacts::default();
            let mut medium = MountedStateMedium::<card::Port<'_>>::new(io, None);
            let mut native = card.port(io);
            let mut store = ReadingStateRouter::new(nvs::NvsBlobBackend::new(&mut flash),
                &mut state, &mut medium, &mut native);
            assert_eq!(ExternalProgressStore::new(&mut store, Some(&digest)).load(&mut restored),
                Load::Loaded(()));
        }
        assert_eq!(restored.locator, saved.locator);
        {
            use newt_reader_ui::{PhysicalOwner, RestoreFailure};
            let mut native = card.port(io);
            let mut state = ChapterCacheState::new();
            let mut pages = reader_pages::ReaderPages(ChapterCacheStorage::lend(&mut state, &mut native, &route).unwrap());
            let mut owner = ExternalOwner::default();
            let mut authorize = |raw: &mut dyn BookSource, expected: &[u8; 32], _: &mut [u8], allowed: &dyn Fn() -> bool| {
                let mut bytes = vec![0; raw.size() as usize];
                let mut actual = [0; 32];
                allowed() && raw.read_at(0, &mut bytes) == bytes.len() as i32
                    && hash(&bytes, &mut actual) && &actual == expected
            };
            let mut source = StrictSource::new(&mut source, &digest).with_reopen_authorizer(&mut authorize, &|| true);
            let mut ports = PhysicalOwner { storage: &mut storage, panel, backend: &mut parser, breaker: &mut breaker,
                cache: PhasedChapterCache { store: &mut pages, hash: &mut Hash, settings },
                open: PhysicalOpen::Import, cache_only: false };
            assert!(!owner.open_physical(&mut reader, &mut source, book, restored,
                RestorePolicy::ExactPage, now(), &mut ports));
            assert_eq!(owner.restore_failure, RestoreFailure::InvalidCheckpoint);
            for policy in [RestorePolicy::Validated, RestorePolicy::ColdCheckpoint] {
                assert!(owner.open_physical(&mut reader, &mut source, book, restored, policy, now(), &mut ports));
                assert_eq!(reader.page_plan().unwrap().text(), expected_text);
                assert_eq!(reader.info().char_start, page.char_start);
            }
            let outside = Progress { locator: Locator { character: u32::MAX, ..restored.locator }, ..restored };
            assert!(!owner.open_physical(&mut reader, &mut source, book, outside,
                RestorePolicy::ColdCheckpoint, now(), &mut ports));
            assert_eq!(owner.restore_failure, RestoreFailure::InvalidCheckpoint);
            pages.0.access().close();
        }
        {
            let mut native = card.port(io);
            let mut state = ChapterCacheState::new();
            let mut pages = reader_pages::ReaderPages(ChapterCacheStorage::lend(&mut state, &mut native, &route).unwrap());
            let mut cache = PhasedChapterCache { store: &mut pages, hash: &mut Hash, settings };
            assert_eq!(reader.open_phased(&mut source, &mut storage, panel, &mut parser), Status::Ok);
            let mut op = OperationPhased::new(&mut reader, &mut storage, panel, &mut parser, &mut breaker);
            let request = CacheRequest { spine: restored.locator.spine, target: restored.locator.character,
                anchor: true, staged: false, build_on_miss: false, cache_only: false };
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
            let started = std::time::Instant::now();
            assert!(cache.capture(&mut source, &digest, request, &mut op).hit);
            println!("host layout capture: miss={}us hit={}us rebuild={}us",
                miss.as_micros(), started.elapsed().as_micros(), rebuild.as_micros());
            pages.0.access().close();
        }
        let after = card.0.borrow();
        assert_eq!(after.model.files[&path].durable, current);
        for (name, bytes) in other {
            assert_eq!(after.model.files[&name].durable, bytes, "{name} changed during Pages rebuild");
        }
    }).unwrap();
    println!("PASS old Pages data misses, saved content resumes, and only Pages rebuilds");
}
