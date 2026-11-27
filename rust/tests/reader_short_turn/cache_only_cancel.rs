// SPDX-License-Identifier: MIT
//! Focused host proof for PhysicalOwner `cache_only` probes and cancelled live
//! turn/jump verified exits (#103).
use super::*;
use newt_core::{
    durable::{Commit, Load},
    external_progress_store::ExternalProgressStore,
    progress::Locator,
    selection_authority::SelectionAuthority,
    zip_container::Status,
};
use newt_native_storage::{
    Authority, RouteState,
    chapter_cache::{ChapterCacheState, ChapterCacheStorage},
};
use newt_phased_reader::{
    PhasedChapterCache,
    imported_revision::{BLOCK_BYTES, Digest, ImportedRevision, RevisionHasher},
};
use newt_reader_ui::{
    PhysicalOwner, ReaderOwnerState, ReaderSource, ReaderUiWork,
};
use upgrade::with_router;

#[derive(Default)]
struct ShaHasher(Vec<u8>);
impl RevisionHasher for ShaHasher {
    fn begin(&mut self) -> bool {
        self.0.clear();
        true
    }
    fn update(&mut self, input: &[u8]) -> bool {
        self.0.extend_from_slice(input);
        true
    }
    fn finish(&mut self, output: &mut Digest) -> bool {
        hash(&self.0, output)
    }
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        hash(input, output)
    }
}

struct FlagCancel<'a>(&'a core::cell::Cell<bool>);
impl Authority for FlagCancel<'_> {
    fn cancelled(&self) -> bool {
        self.0.get()
    }
}

struct CancelSource<'a> {
    bytes: Vec<u8>,
    cancel: &'a core::cell::Cell<bool>,
    cancel_after: &'a core::cell::Cell<u32>,
    reads: &'a core::cell::Cell<u32>,
}
impl BookSource for CancelSource<'_> {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if !out.is_empty() {
            self.reads.set(self.reads.get().saturating_add(1));
        }
        let left = self.cancel_after.get();
        if left > 0 && !out.is_empty() {
            if left == 1 {
                self.cancel_after.set(0);
                self.cancel.set(true);
            } else {
                self.cancel_after.set(left - 1);
            }
        }
        if self.cancel.get() {
            return -1;
        }
        let Ok(at) = usize::try_from(at) else { return -1 };
        let Some(bytes) = self.bytes.get(at..) else { return -1 };
        let count = bytes.len().min(out.len());
        out[..count].copy_from_slice(&bytes[..count]);
        count as i32
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for &(name, data) in entries {
        let offset = local.len() as u32;
        let crc = crc32(data);
        let len = data.len() as u32;
        let nlen = name.len() as u16;
        local.extend_from_slice(&0x04034b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&[0u8; 8]);
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&len.to_le_bytes());
        local.extend_from_slice(&len.to_le_bytes());
        local.extend_from_slice(&nlen.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name.as_bytes());
        local.extend_from_slice(data);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&[0u8; 8]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&nlen.to_le_bytes());
        central.extend_from_slice(&[0u8; 12]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = local.len() as u32;
    let cd_size = central.len() as u32;
    let count = entries.len() as u16;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn synthetic_fragment_epub() -> Vec<u8> {
    synthetic_epub(false)
}

fn synthetic_epub(two_chapters: bool) -> Vec<u8> {
    let mut ch = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Ch 1</title></head><body><h1>Chapter 1</h1>",
    );
    for i in 0..45 {
        if i == 25 {
            ch.push_str("<h2 id=\"mid\">Middle Section</h2>");
        }
        ch.push_str(&format!(
            "<p>Paragraph {i}: A lantern glowed beside the quiet harbor path while cedar boats rocked along the wooden pier.</p>"
        ));
    }
    ch.push_str("</body></html>");
    let mut ch1 = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Ch 2</title></head><body><h1>Chapter 2</h1>",
    );
    for i in 0..16 {
        ch1.push_str(&format!(
            "<p>Second chapter paragraph {i}: Morning light crossed the breakwater and warmed the stone quay.</p>"
        ));
    }
    ch1.push_str("</body></html>");
    let nav: &[u8] = if two_chapters {
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\
        <head><title>Contents</title></head><body><nav epub:type=\"toc\"><ol>\
        <li><a href=\"c0.xhtml\">Chapter 1</a></li>\
        <li><a href=\"c0.xhtml#mid\">Middle Section</a></li>\
        <li><a href=\"c1.xhtml\">Chapter 2</a></li>\
        </ol></nav></body></html>"
    } else {
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\
        <head><title>Contents</title></head><body><nav epub:type=\"toc\"><ol>\
        <li><a href=\"c0.xhtml\">Chapter 1</a></li>\
        <li><a href=\"c0.xhtml#mid\">Middle Section</a></li>\
        </ol></nav></body></html>"
    };
    let container = b"<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
        <rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
    let opf: &[u8] = if two_chapters {
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"u\">\
        <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
        <dc:identifier id=\"u\">urn:newt-test:103-cancel-cc0</dc:identifier>\
        <dc:title>Harbor Cancel</dc:title><dc:language>en</dc:language></metadata>\
        <manifest>\
        <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\
        <item id=\"c0\" href=\"c0.xhtml\" media-type=\"application/xhtml+xml\"/>\
        <item id=\"c1\" href=\"c1.xhtml\" media-type=\"application/xhtml+xml\"/>\
        </manifest><spine><itemref idref=\"c0\"/><itemref idref=\"c1\"/></spine></package>"
    } else {
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"u\">\
        <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
        <dc:identifier id=\"u\">urn:newt-test:103-cancel-cc0</dc:identifier>\
        <dc:title>Harbor Cancel</dc:title><dc:language>en</dc:language></metadata>\
        <manifest>\
        <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\
        <item id=\"c0\" href=\"c0.xhtml\" media-type=\"application/xhtml+xml\"/>\
        </manifest><spine><itemref idref=\"c0\"/></spine></package>"
    };
    if two_chapters {
        stored_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", container),
            ("OEBPS/content.opf", opf),
            ("OEBPS/nav.xhtml", nav),
            ("OEBPS/c0.xhtml", ch.as_bytes()),
            ("OEBPS/c1.xhtml", ch1.as_bytes()),
        ])
    } else {
        stored_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", container),
            ("OEBPS/content.opf", opf),
            ("OEBPS/nav.xhtml", nav),
            ("OEBPS/c0.xhtml", ch.as_bytes()),
        ])
    }
}

pub fn check() {
    let epub = synthetic_fragment_epub();
    let mut digest = [0u8; 32];
    assert!(hash(&epub, &mut digest));
    let book = u64::from_le_bytes(digest[..8].try_into().unwrap());

    let mut tokens = [0u8; 2];
    // SAFETY: the existing host Board fake treats these as disjoint tokens.
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);

    board
        .with_reader_io(|panel, io| {
            let mut card = card::Card::default();
            let cancel_flag = core::cell::Cell::new(false);
            let cancel_after = core::cell::Cell::new(0u32);
            let reads = core::cell::Cell::new(0u32);
            let cancel_auth = FlagCancel(&cancel_flag);
            let route_state = RouteState::new(
                reader_book::CACHE_ROOT,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
            let route = route_state.lend(&cancel_auth, now);

            let mut native_pages = card.port(io);
            let mut pages_state = ChapterCacheState::new();
            let mut pages = reader_pages::ReaderPages(
                ChapterCacheStorage::lend(&mut pages_state, &mut native_pages, &route).unwrap(),
            );

            let mut native_receipt = card.port(io);
            let mut receipt_state = sd_revision::SdRevisionState::new(&mut native_receipt, &route);
            let mut receipt = sd_revision::SdRevisionStorage::lend(
                &mut receipt_state,
                &mut native_receipt,
                &route,
            )
            .unwrap();

            let mut rev_hash = ShaHasher::default();
            let mut rev_scratch = [0u8; BLOCK_BYTES];
            let mut raw = CancelSource { bytes: epub.clone(), cancel: &cancel_flag, cancel_after: &cancel_after, reads: &reads };
            let mut revision = ImportedRevision::full(&mut receipt, &mut rev_hash, &mut rev_scratch);
            assert!(revision.begin_import(&mut raw));
            let mut source = ReaderSource::new(revision);

            let mut bank = [0u8; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = linebreak::NativeBreak;
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [Run::default(); RUNS];
            let mut text = [0u8; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let settings = ViewSettings {
                orientation: newt_core::preferences::Orientation::Portrait,
                ..ViewSettings::default()
            };
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), settings.page_layout());
            let mut owner = ExternalOwner::default();
            let mut ui = ReaderUiState::default();
            let saved = Progress { book, ..Progress::default() };

            // Open with PhysicalOpen::Prepared so the receipt seals on SD.
            let mut ports = PhysicalOwner {
                storage: &mut storage,
                panel,
                backend: &mut parser,
                breaker: &mut breaker,
                cache: PhasedChapterCache { store: &mut pages, hash: &mut Hash, settings },
                open: PhysicalOpen::Prepared,
                cache_only: false,
            };
            let indexed = newt_phased_reader::index_build::open(
                &mut source,
                &digest,
                ports.storage,
                ports.panel,
                ports.backend,
                ports.cache.store,
                None,
                newt_phased_reader::index_build::Verification::Verified,
            )
            .unwrap();
            assert!(owner.open_physical(
                &indexed,
                &mut reader,
                &mut source,
                book,
                saved,
                RestorePolicy::AllowInitialFallback,
                1_000,
                &mut ports,
            ));
            let page0 = reader.info();
            assert!(page0.page_count >= 4, "expected multi-page chapter, got {}", page0.page_count);

            // 1a. cache_only cold miss on Revision mode must not lay out or advance.
            reader.reset_layout_pages();
            ports.cache_only = true;
            let handled = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Next, 1_100, &mut ports,
            );
            let done = owner.complete_ui_physical(
                &mut reader, &mut source, &mut ui, handled, 1_100, &mut ports,
            );
            assert_eq!(
                done.work,
                ReaderUiWork::Intent(ReaderUiIntent::None),
                "cache_only cold miss must not publish a turn"
            );
            assert_eq!(reader.info(), page0, "cache_only cold miss must preserve current page");
            assert_eq!(reader.last_layout_pages(), 0, "cache_only cold miss must lay out zero pages");
            assert!(owner.state.opened(), "cache_only cold miss must keep reader open");

            // 1b. cache_only cold miss on PendingReaderSource must neither lay out nor finish verification.
            let mut finish_called = false;
            {
                let mut finish = |_: &mut dyn BookSource, _: &Digest, _: &mut [u8], _: &dyn Fn() -> bool| {
                    finish_called = true;
                    true
                };
                let mut pending = source.with_verification_gate(0, &mut finish, &|| true);
                reader.reset_layout_pages();
                let handled = owner.handle_ui_physical(
                    &mut reader, &mut pending, &mut ui, settings, Action::Next, 1_200, &mut ports,
                );
                let done = owner.complete_ui_physical(
                    &mut reader, &mut pending, &mut ui, handled, 1_200, &mut ports,
                );
                assert_eq!(
                    done.work,
                    ReaderUiWork::Intent(ReaderUiIntent::None),
                    "pending cache_only cold miss must defer without layout"
                );
                assert_eq!(reader.info(), page0);
                assert_eq!(reader.last_layout_pages(), 0, "pending cache_only cold miss must lay out zero pages");
                assert!(!finish_called, "pending cache_only cold miss must not run full-source verification");
            }

            // 1c. cache_only cold miss on StrictSource must not lay out or advance.
            {
                let mut selection = SelectionAuthority::new();
                assert!(selection.publish_candidate(c"generated.epub", digest, &mut || {}, &mut || {}));
                let mut raw_strict = Source(epub.clone());
                let mut authorize = |_: &mut dyn BookSource, _: &Digest, _: &mut [u8], _: &dyn Fn() -> bool| true;
                let mut strict = StrictSource::for_live_reader(&mut raw_strict, &selection, &owner, &mut reader)
                    .expect("live strict source")
                    .with_reopen_authorizer(&mut authorize, &|| true);
                reader.reset_layout_pages();
                let handled = owner.handle_ui_physical(
                    &mut reader, &mut strict, &mut ui, settings, Action::Next, 1_250, &mut ports,
                );
                let done = owner.complete_ui_physical(
                    &mut reader, &mut strict, &mut ui, handled, 1_250, &mut ports,
                );
                assert_eq!(
                    done.work,
                    ReaderUiWork::Intent(ReaderUiIntent::None),
                    "strict cache_only cold miss must not lay out"
                );
                assert_eq!(reader.info(), page0);
                assert_eq!(reader.last_layout_pages(), 0, "strict cache_only cold miss must lay out zero pages");
            }

            // 1d. Populate chapter 0 page cache via the real producer; cache_only hit must advance
            //     with 0 layout pages for both PendingReaderSource (without verifying) and Revision mode.
            {
                use newt_core::producer_policy::{Cancellation, Hooks, Stop};
                fn never_stop(_: &()) -> Stop { Stop::None }
                let cancel = Cancellation::new(Hooks {
                    context: &(),
                    stop: Some(never_stop),
                    yield_now: None,
                    pause: None,
                });
                let info = reader.info();
                let mut route = newt_phased_reader::producer::Route {
                    settings,
                    digest: &digest,
                    info,
                    hash: ports.cache.hash,
                };
                let produced = ports.storage.with_producer(
                    ports.panel,
                    &cancel,
                    ports.cache.store,
                    &mut source,
                    info.page_count,
                    None,
                    |producer| {
                        Ok(producer.produce_current(&mut reader, ports.backend, ports.breaker, &mut route))
                    },
                ).unwrap();
                assert_eq!(produced, newt_core::producer_visit::CurrentOutcome::Done);
            }
            {
                let mut finish_called = false;
                let mut finish = |_: &mut dyn BookSource, _: &Digest, _: &mut [u8], _: &dyn Fn() -> bool| {
                    finish_called = true;
                    true
                };
                let mut pending = source.with_verification_gate(0, &mut finish, &|| true);
                reader.reset_layout_pages();
                let handled = owner.handle_ui_physical(
                    &mut reader, &mut pending, &mut ui, settings, Action::Next, 1_300, &mut ports,
                );
                let done = owner.complete_ui_physical(
                    &mut reader, &mut pending, &mut ui, handled, 1_300, &mut ports,
                );
                assert_eq!(
                    done.work,
                    ReaderUiWork::Intent(ReaderUiIntent::Repaint),
                    "pending cache_only hit must advance"
                );
                assert_eq!(reader.info().page, 1);
                assert_eq!(reader.last_layout_pages(), 0, "pending cache_only hit must lay out zero pages");
                assert!(!finish_called);
            }
            reader.reset_layout_pages();
            let handled = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Next, 1_400, &mut ports,
            );
            let done = owner.complete_ui_physical(
                &mut reader, &mut source, &mut ui, handled, 1_400, &mut ports,
            );
            assert_eq!(
                done.work,
                ReaderUiWork::Intent(ReaderUiIntent::Repaint),
                "revision cache_only hit must advance"
            );
            let page2 = reader.info();
            assert_eq!(page2.page, 2);
            assert!(page2.char_start > 0, "page 2 must have nonzero char_start");
            assert_eq!(reader.last_layout_pages(), 0, "revision cache_only hit must lay out zero pages");

            // 2. Cancelled live action exit: prepare chapter menu, then cancel a fragment jump
            //    so finish_reader_action closes the receipt handle, then verify reload_closed_receipt
            //    and finish_exit_captured(allow_cancelled=true) save unsaved page 2 cleanly.
            ports.cache_only = false;
            ui.panel = ReaderPanel::Menu;
            ui.selected = 1; // Contents
            let handled = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Activate, 2_000, &mut ports,
            );
            let _ = owner.complete_ui_physical(
                &mut reader, &mut source, &mut ui, handled, 2_000, &mut ports,
            );
            assert_eq!(ui.panel, ReaderPanel::Chapters);
            assert!(reader.chapter_menu().info().count >= 2);
            ui.selected = 1; // "Middle Section" (c0.xhtml#mid fragment!)

            // Cancel mid-jump (after initial reads set deferred_fresh) so finish_deferred_path_checks
            // trips on the refused freshness read and closes the receipt handle.
            cancel_after.set(2);
            let handled = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Activate, 5_000, &mut ports,
            );
            let _ = owner.complete_ui_physical(
                &mut reader, &mut source, &mut ui, handled, 5_000, &mut ports,
            );
            assert!(cancel_flag.get(), "mid-jump cancel must fire");
            cancel_flag.set(false);
            // Cancelled fragment jump closed the receipt handle on #103.
            assert_eq!(source.digest(), [0u8; 32], "cancelled fragment jump closed receipt handle");

            // Mirror cancel_live_action_image: end reading interval at action_at and mark IoError
            // while retaining has_progress.
            owner.leave(5_000);
            owner.state = ReaderOwnerState::from_open(
                owner.state.has_progress(),
                Status::IoError,
                owner.state.readable_spines(),
            );
            assert!(!owner.state.opened() && owner.state.has_progress());

            // 2a. StrictSource::for_live_reader refuses cancelled state; for_cancelled_exit accepts it.
            let mut selection = SelectionAuthority::new();
            assert!(selection.publish_candidate(c"generated.epub", digest, &mut || {}, &mut || {}));
            let mut raw_strict = Source(epub.clone());
            assert!(
                StrictSource::for_live_reader(&mut raw_strict, &selection, &owner, &mut reader).is_none(),
                "for_live_reader must reject cancelled owner state"
            );
            assert!(
                StrictSource::for_cancelled_exit(&mut raw_strict, &selection, &owner, &mut reader).is_some(),
                "for_cancelled_exit must accept cancelled owner state with progress"
            );

            // 2b. reload_closed_receipt rejects a wrong digest and succeeds for expected digest.
            let mut wrong = digest;
            wrong[0] ^= 0xff;
            assert!(
                !source.reload_closed_receipt(wrong),
                "reload_closed_receipt must fail closed on wrong digest"
            );
            assert_eq!(source.digest(), [0u8; 32]);
            assert!(
                source.reload_closed_receipt(digest),
                "reload_closed_receipt must restore matching stored receipt"
            );
            assert_eq!(source.digest(), digest);

            // Seed selection on SD so MountedCheckpoint can commit progress + stats.
            let selected_record = newt_core::sd_resume::SdResumeRecord::new(c"generated.epub", digest).unwrap();
            let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
            with_router(&mut card, io, |store| {
                assert_eq!(
                    newt_core::selected_book::SelectedBookStore::new(store).save(selected_record),
                    Commit::Saved
                );
                checkpoint.selection_committed(store, &digest, false, false);
            });

            // 2c. Negative control: allow_cancelled = false refuses to save cancelled owner state.
            let mut flash = Flash;
            let mut facts = newt_native_storage::reading_state::StateFacts::default();
            let mut statistics = ReadingStatsRuntime::default();
            let mut accepted_owed = true;
            let cancel_exit = reader_book::ExitRequest {
                zone: newt_core::time_zone::TimeZone::Utc,
                cancel: &Never,
            };
            drop(ports);
            let mut resources = Resources {
                panel,
                pages: &mut pages,
                storage: &mut storage,
                parser: newt_target_expat::Expat::default(),
                breaker: &mut breaker,
                hash: &mut Hash,
                settings,
                open: PhysicalOpen::Selected { digest, spine: page2.spine },
                cache_only: false,
                recovery: None,
            };
            {
                let mut mounted = MountedCheckpoint::new(&mut checkpoint, &mut flash, &mut facts);
                mounted.finish_exit_captured(
                    io, &mut card, &mut resources, &reader, &source,
                    &mut owner, &mut ui, digest, &cancel_exit, &hash,
                    &mut statistics, 5_000, &mut accepted_owed, false, false, None,
                );
            }
            assert_ne!(
                checkpoint.result().progress,
                reader_book::checkpoint::Outcome::Verified,
                "allow_cancelled=false must not commit a cancelled owner state"
            );

            // 2d. Positive: allow_cancelled = true commits unsaved page 2 to durable SD!
            checkpoint.begin_operation();
            {
                let mut mounted = MountedCheckpoint::new(&mut checkpoint, &mut flash, &mut facts);
                mounted.finish_exit_captured(
                    io, &mut card, &mut resources, &reader, &source,
                    &mut owner, &mut ui, digest, &cancel_exit, &hash,
                    &mut statistics, 5_000, &mut accepted_owed, false, true, None,
                );
            }
            assert_eq!(
                checkpoint.result().progress,
                reader_book::checkpoint::Outcome::Verified,
                "cancelled live exit must verify and commit unsaved progress"
            );
            drop(resources);
            drop(source);
            drop(receipt);
            let _ = receipt_state.close(&mut native_receipt, &route);
            pages.0.access().close();
            drop(pages);
            let _ = pages_state.close(&mut native_pages, &route);
            let mut cold = card.cold_bytes();
            let mut restored = Progress::default();
            assert_eq!(
                with_router(&mut cold, io, |store| {
                    ExternalProgressStore::new(store, Some(&digest)).load(&mut restored)
                }),
                Load::Loaded(()),
                "unsaved progress must survive cold reload after cancelled exit"
            );
            assert_eq!(
                restored.locator,
                Locator { spine: page2.spine, character: page2.char_start },
                "cancelled live exit must persist exact unsaved pre-turn locator"
            );
        })
        .unwrap();

    check_cross_chapter_cache_only();

    println!("PASS cache_only cold-miss/hit and cancelled live exit save/reload");
}

fn check_cross_chapter_cache_only() {
    let epub = synthetic_epub(true);
    let mut digest = [0u8; 32];
    assert!(hash(&epub, &mut digest));
    let book = u64::from_le_bytes(digest[..8].try_into().unwrap());

    let mut tokens = [0u8; 2];
    // SAFETY: the existing host Board fake treats these as disjoint tokens.
    let (mut board, pending, _) = unsafe {
        newt_c3_board::Board::acquire_x3_optional_sd(
            (&mut tokens[0] as *mut u8).cast(),
            (&mut tokens[1] as *mut u8).cast(),
        )
    }
    .unwrap();
    drop(pending);

    board
        .with_reader_io(|panel, io| {
            let mut card = card::Card::default();
            let cancel_flag = core::cell::Cell::new(false);
            let cancel_after = core::cell::Cell::new(0u32);
            let reads = core::cell::Cell::new(0u32);
            let cancel_auth = FlagCancel(&cancel_flag);
            let route_state = RouteState::new(
                reader_book::CACHE_ROOT,
                "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
            );
            let route = route_state.lend(&cancel_auth, now);

            let mut native_pages = card.port(io);
            let mut pages_state = ChapterCacheState::new();
            let mut pages = reader_pages::ReaderPages(
                ChapterCacheStorage::lend(&mut pages_state, &mut native_pages, &route).unwrap(),
            );

            let mut native_receipt = card.port(io);
            let mut receipt_state = sd_revision::SdRevisionState::new(&mut native_receipt, &route);
            let mut receipt = sd_revision::SdRevisionStorage::lend(
                &mut receipt_state,
                &mut native_receipt,
                &route,
            )
            .unwrap();

            let mut rev_hash = ShaHasher::default();
            let mut rev_scratch = [0u8; BLOCK_BYTES];
            let mut raw = CancelSource { bytes: epub.clone(), cancel: &cancel_flag, cancel_after: &cancel_after, reads: &reads };
            let mut revision = ImportedRevision::full(&mut receipt, &mut rev_hash, &mut rev_scratch);
            assert!(revision.begin_import(&mut raw));
            let mut source = ReaderSource::new(revision);

            let mut bank = [0u8; BANK_BYTES];
            let mut storage = Storage::new(&mut bank);
            let mut parser = newt_target_expat::Expat::default();
            let mut breaker = linebreak::NativeBreak;
            let mut selections = [Selection::default(), Selection::default()];
            let mut runs = [Run::default(); RUNS];
            let mut text = [0u8; TEXT];
            let mut plan = Plan::new(&mut runs, &mut text);
            let settings = ViewSettings {
                orientation: newt_core::preferences::Orientation::Portrait,
                ..ViewSettings::default()
            };
            let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), settings.page_layout());
            let mut owner = ExternalOwner::default();
            let mut ui = ReaderUiState::default();
            let saved = Progress { book, ..Progress::default() };

            let mut ports = PhysicalOwner {
                storage: &mut storage,
                panel,
                backend: &mut parser,
                breaker: &mut breaker,
                cache: PhasedChapterCache { store: &mut pages, hash: &mut Hash, settings },
                open: PhysicalOpen::Prepared,
                cache_only: false,
            };
            let indexed = newt_phased_reader::index_build::open(
                &mut source,
                &digest,
                ports.storage,
                ports.panel,
                ports.backend,
                ports.cache.store,
                None,
                newt_phased_reader::index_build::Verification::Verified,
            )
            .unwrap();
            assert!(owner.open_physical(
                &indexed, &mut reader, &mut source, book, saved,
                RestorePolicy::AllowInitialFallback, 1_000, &mut ports,
            ));

            // Advance into chapter 1 and populate chapter 1's page cache on SD.
            while reader.info().spine == 0 {
                let h = owner.handle_ui_physical(
                    &mut reader, &mut source, &mut ui, settings, Action::Next, 1_100, &mut ports,
                );
                let _ = owner.complete_ui_physical(
                    &mut reader, &mut source, &mut ui, h, 1_100, &mut ports,
                );
            }
            assert_eq!(reader.info().spine, 1);
            {
                use newt_core::producer_policy::{Cancellation, Hooks, Stop};
                fn never_stop(_: &()) -> Stop { Stop::None }
                let cancel = Cancellation::new(Hooks {
                    context: &(), stop: Some(never_stop), yield_now: None, pause: None,
                });
                let ch1_info = reader.info();
                let mut route = newt_phased_reader::producer::Route {
                    settings, digest: &digest, info: ch1_info, hash: ports.cache.hash,
                };
                let produced = ports.storage.with_producer(
                    ports.panel, &cancel, ports.cache.store, &mut source, ch1_info.page_count, None,
                    |producer| Ok(producer.produce_current(&mut reader, ports.backend, ports.breaker, &mut route)),
                ).unwrap();
                assert_eq!(produced, newt_core::producer_visit::CurrentOutcome::Done);
            }
            // Return to the last page of chapter 0.
            let h = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Previous, 1_200, &mut ports,
            );
            let _ = owner.complete_ui_physical(
                &mut reader, &mut source, &mut ui, h, 1_200, &mut ports,
            );
            let last_ch0 = reader.info();
            assert_eq!(last_ch0.spine, 0);
            assert_eq!(last_ch0.page + 1, last_ch0.page_count);

            // Cross-chapter cache_only probe must defer BEFORE verifying chapter 1's range.
            ports.cache_only = true;
            reads.set(0);
            reader.reset_layout_pages();
            let h = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Next, 1_300, &mut ports,
            );
            let deferred = owner.complete_ui_physical(
                &mut reader, &mut source, &mut ui, h, 1_300, &mut ports,
            );
            assert_eq!(
                deferred.work,
                ReaderUiWork::Intent(ReaderUiIntent::None),
                "cross-chapter cache_only probe must defer until Loading is shown"
            );
            assert_eq!(reader.info(), last_ch0, "cross-chapter cache_only must keep chapter 0 shown");
            assert_eq!(
                reads.get(),
                0,
                "cross-chapter cache_only must not verify target chapter range before Loading"
            );
            assert_eq!(reader.last_layout_pages(), 0);

            // After Loading is acknowledged (cache_only = false), target chapter range IS verified
            // before publishing the cached page (still 0 layout pages).
            ports.cache_only = false;
            reads.set(0);
            reader.reset_layout_pages();
            let h = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Next, 1_400, &mut ports,
            );
            let published = owner.complete_ui_physical(
                &mut reader, &mut source, &mut ui, h, 1_400, &mut ports,
            );
            assert_eq!(
                published.work,
                ReaderUiWork::Intent(ReaderUiIntent::Repaint),
                "cross-chapter retry after Loading must publish chapter 1"
            );
            assert_eq!(reader.info().spine, 1);
            assert_eq!(reader.info().page, 0);
            assert!(
                reads.get() > 0,
                "cross-chapter retry must verify target chapter range before publishing"
            );
            assert_eq!(reader.last_layout_pages(), 0, "verified cross-chapter retry must hit page cache");

            drop(ports);
            drop(source);
            drop(receipt);
            let _ = receipt_state.close(&mut native_receipt, &route);
            pages.0.access().close();
            drop(pages);
            let _ = pages_state.close(&mut native_pages, &route);
        })
        .unwrap();
}
