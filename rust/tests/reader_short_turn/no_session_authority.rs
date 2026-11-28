// SPDX-License-Identifier: MIT
//! #199: the no-session route's one verification authority. Each operation
//! lends a strict view of a freshly lent descriptor (reader_image_operation.rs);
//! the owner keeps the book's check between loans, and only a successful hash
//! makes a view Verified. The exit passes the same gate (reader_image_exit.rs).
use super::*;

/// One no-session operation: a strict view of the lent descriptor for `$body`,
/// handed back to the owner afterwards.
macro_rules! loan {
    ($owner:ident, $reader:ident, $raw:expr, $selection:ident, $authorize:ident, $allowed:ident,
     $strict:ident => $body:block) => {{
        let mut raw = $raw;
        let mut $strict = StrictSource::for_live_reader(&mut raw, &$selection, &mut $owner, &$reader)
            .expect("a live strict loan").with_reopen_authorizer(&mut $authorize, &$allowed);
        let result = $body;
        $owner.settle_strict_loan($strict);
        result
    }};
}

/// A jump runs out of time, so the strict view must hash the book again. On
/// Back the exit's hash finds the book `changed` (or not). A changed book never
/// saves, and no later loan of this open turns that failure into a verified
/// view: Power after it neither hashes nor saves, even once the bytes read
/// back right again. The unchanged sibling saves the shown page.
pub(super) fn check_timed_out_exit(changed: bool) {
    let epub = synthetic_fragment_epub();
    let mut digest = [0u8; 32];
    assert!(hash(&epub, &mut digest));
    let book = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let mut tokens = [0u8; 2];
    // SAFETY: the existing host Board fake treats these as disjoint tokens.
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(), (&mut tokens[1] as *mut u8).cast())
    }.unwrap();
    drop(pending);

    board.with_reader_io(|panel, io| {
        let mut card = card::Card::default();
        let cancel_flag = core::cell::Cell::new(false);
        let cancel_after = core::cell::Cell::new(0u32);
        let reads = core::cell::Cell::new(0u32);
        let corrupt_at = core::cell::Cell::new(None);
        let route_cancel = core::cell::Cell::new(false);
        let route_queries = core::cell::Cell::new(0u32);
        let cancel_auth = FlagCancel(&cancel_flag, &route_cancel, &route_queries);
        let route_state = RouteState::new(reader_book::CACHE_ROOT,
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff");
        let route = route_state.lend(&cancel_auth, now);
        let mut native_pages = card.port(io);
        let mut pages_state = ChapterCacheState::new();
        let mut pages = reader_pages::ReaderPages(
            ChapterCacheStorage::lend(&mut pages_state, &mut native_pages, &route).unwrap());
        // Each loan relends the selected descriptor, as firmware does.
        let lend = || CancelSource { bytes: epub.clone(), cancel: &cancel_flag, cancel_after: &cancel_after,
            reads: &reads, corrupt_at: &corrupt_at };
        let hashes = core::cell::Cell::new(0u32);
        let mut authorize = |raw: &mut dyn BookSource, expected: &Digest, _: &mut [u8], allowed: &dyn Fn() -> bool| {
            hashes.set(hashes.get() + 1);
            let mut bytes = vec![0; raw.size() as usize];
            let mut actual = [0; 32];
            allowed() && raw.read_at(0, &mut bytes) == bytes.len() as i32 && hash(&bytes, &mut actual)
                && actual == *expected
        };
        let allowed = || !cancel_flag.get();
        let mut selection = SelectionAuthority::new();
        assert!(selection.publish_candidate(c"generated.epub", digest, &mut || {}, &mut || {}));

        let mut bank = [0u8; BANK_BYTES];
        let mut storage = Storage::new(&mut bank);
        let mut parser = newt_target_expat::Expat::default();
        let mut breaker = linebreak::NativeBreak;
        let mut selections = [Selection::default(), Selection::default()];
        let mut runs = [Run::default(); RUNS];
        let mut text = [0u8; TEXT];
        let mut plan = Plan::new(&mut runs, &mut text);
        let settings = ViewSettings { orientation: newt_core::preferences::Orientation::Portrait,
            ..ViewSettings::default() };
        let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), settings.page_layout());
        let mut owner = ExternalOwner::default();
        let mut ui = ReaderUiState::default();
        let mut ports = PhysicalOwner { storage: &mut storage, panel, backend: &mut parser,
            breaker: &mut breaker, cache: PhasedChapterCache { store: &mut pages, hash: &mut Hash, settings },
            open: PhysicalOpen::Prepared, cache_only: false };
        // The open hashed this descriptor before its index pass; its first loan
        // starts from that pass.
        let mut raw = lend();
        let indexed = newt_phased_reader::index_build::open(&mut raw, &digest, ports.storage, ports.panel,
            ports.backend, ports.cache.store, None, newt_phased_reader::index_build::Verification::Verified,
        ).unwrap();
        let mut strict = StrictSource::from_indexed(&mut raw, &indexed).with_reopen_authorizer(&mut authorize, &allowed);
        assert!(owner.open_physical(&indexed, &mut reader, &mut strict, book, Progress { book, ..Progress::default() },
            RestorePolicy::AllowInitialFallback, 1_000, &mut ports), "the no-session open: {:?}", owner.state.status());
        owner.settle_strict_loan(strict);
        let page0 = reader.info();

        // A cancel during the new descriptor's availability probe is a cancel:
        // the probe runs inside the action's gate, so it keeps the reader.
        loan!(owner, reader, lend(), selection, authorize, allowed, strict => {
            cancel_flag.set(true);
            let handled = owner.handle_ui_physical(&mut reader, &mut strict, &mut ui, settings, Action::Next, 1_100, &mut ports);
            let _ = owner.complete_ui_physical(&mut reader, &mut strict, &mut ui, handled, 1_100, &mut ports);
            cancel_flag.set(false);
        });
        assert!(owner.state.status() == Status::Ok && reader.opened() && reader.info() == page0,
            "a cancelled availability probe keeps the reader and its page");
        loan!(owner, reader, lend(), selection, authorize, allowed, strict => {
            let handled = owner.handle_ui_physical(&mut reader, &mut strict, &mut ui, settings, Action::Next, 1_200, &mut ports);
            let _ = owner.complete_ui_physical(&mut reader, &mut strict, &mut ui, handled, 1_200, &mut ports);
        });
        let shown = reader.info();
        assert!(owner.state.status() == Status::Ok && shown.page == page0.page + 1 && hashes.get() == 0,
            "the next loan probes its descriptor and turns without hashing");

        loan!(owner, reader, lend(), selection, authorize, allowed, strict => {
            jump_to_middle!(owner, reader, strict, ui, settings, ports, 1_300, { cancel_after.set(2); });
        });
        assert!(cancel_flag.replace(false), "the jump must run out of time");
        assert!(owner.state.status() == Status::Ok && reader.opened() && reader.info() == shown,
            "a timed-out no-session jump keeps the reader and its page");
        // Firmware's cancel_live_action_image: the timed-out action leaves the
        // owner unopened with its progress, for the queued Back or Power.
        owner.leave(1_400);
        owner.state = ReaderOwnerState::from_open(owner.state.has_progress(), Status::IoError,
            owner.state.readable_spines());

        let selected = newt_core::sd_resume::SdResumeRecord::new(c"generated.epub", digest).unwrap();
        let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
        with_router(&mut card, io, |store| {
            assert_eq!(newt_core::selected_book::SelectedBookStore::new(store).save(selected), Commit::Saved);
            checkpoint.selection_committed(store, &digest, false, false);
        });
        drop(ports);
        let mut flash = Flash;
        let mut facts = newt_native_storage::reading_state::StateFacts::default();
        let mut statistics = ReadingStatsRuntime::default();
        let mut accepted_owed = true;
        let request = reader_book::ExitRequest { zone: newt_core::time_zone::TimeZone::Utc, cancel: &Never };
        let mut resources = Resources { panel, pages: &mut pages, storage: &mut storage,
            parser: newt_target_expat::Expat::default(), breaker: &mut breaker, hash: &mut Hash, settings,
            open: PhysicalOpen::Selected { digest, spine: shown.spine }, cache_only: false, recovery: None };
        // Back, then Power after a refused Back, as reader_image_exit.rs runs
        // them: the exit passes the gate before it saves.
        for (sleeping, at) in [(false, 1_500), (true, 1_600)] {
            if sleeping && !changed { break; }
            corrupt_at.set((changed && !sleeping).then_some(epub.len() / 2));
            let before = hashes.get();
            let mut capture = crate::reader_exit_metadata::ExitMetadata::new();
            checkpoint.begin_operation();
            let mut raw = lend();
            let mut strict = StrictSource::for_cancelled_exit(&mut raw, &selection, &mut owner, &reader)
                .expect("the timed-out exit's strict loan").with_reopen_authorizer(&mut authorize, &allowed);
            let reloaded = strict.reload_closed(&mut reader, &mut []);
            {
                let mut mounted = MountedCheckpoint::new(&mut checkpoint, &mut flash, &mut facts);
                mounted.finish_exit_captured(io, &mut card, &mut resources, &reader, &strict, &mut owner, &mut ui,
                    digest, &request, &hash, &mut statistics, at, &mut accepted_owed, sleeping, true,
                    Some(&mut capture));
            }
            owner.settle_strict_loan(strict);
            let saved = checkpoint.result().progress == reader_book::checkpoint::Outcome::Verified;
            if !changed {
                assert!(reloaded && saved && hashes.get() == before + 1, "a timed-out exit hashes the book again and saves");
                assert!(capture.identified && capture.locator == Locator { spine: shown.spine, character: shown.char_start },
                    "a timed-out exit that saves keeps the shown page as its destination");
            } else if !sleeping {
                assert!(!reloaded && !saved && !capture.identified && hashes.get() == before + 1,
                    "a failed re-hash on exit saves no checkpoint and no destination");
            } else {
                assert!(!reloaded && !saved && !capture.identified && hashes.get() == before,
                    "a failed re-hash stays failed for the open: a later exit neither hashes nor saves");
            }
        }
        corrupt_at.set(None);
        drop(resources);
        pages.0.access().close();
        drop(pages);
        let _ = pages_state.close(&mut native_pages, &route);
        let mut cold = card.cold_bytes();
        let mut restored = Progress::default();
        let load = with_router(&mut cold, io, |store| {
            ExternalProgressStore::new(store, Some(&digest)).load(&mut restored)
        });
        if changed {
            assert_ne!(load, Load::Loaded(()), "a changed book's exits leave no durable checkpoint");
        } else {
            assert_eq!((load, restored.locator), (Load::Loaded(()), Locator { spine: shown.spine, character: shown.char_start }),
                "a cold boot restores the page the timed-out exit saved");
        }
    }).unwrap();
}
