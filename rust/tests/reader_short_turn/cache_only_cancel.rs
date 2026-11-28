// SPDX-License-Identifier: MIT
//! Focused host proof for PhysicalOwner `cache_only` probes, cancelled live
//! turn/jump verified exits (#103) and cancelled Contents jumps (#199).
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
    PhysicalOwner, ReaderOwnerState, ReaderSource, ReaderUiNotice, ReaderUiWork,
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

/// The operation's cancel: the shared flag, which the source also obeys, or
/// a route-only cancel that leaves the source readable.
struct FlagCancel<'a>(&'a core::cell::Cell<bool>, &'a core::cell::Cell<bool>);
impl Authority for FlagCancel<'_> {
    fn cancelled(&self) -> bool {
        self.0.get() || self.1.get()
    }
}

struct CancelSource<'a> {
    bytes: Vec<u8>,
    cancel: &'a core::cell::Cell<bool>,
    cancel_after: &'a core::cell::Cell<u32>,
    reads: &'a core::cell::Cell<u32>,
    /// Cancels only the route on this many more reads; the source keeps serving them.
    route_after: &'a core::cell::Cell<u32>,
    route_cancel: &'a core::cell::Cell<bool>,
    /// A source offset whose byte reads back flipped, as a corrupt SD block.
    corrupt_at: &'a core::cell::Cell<Option<usize>>,
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
        let left = self.route_after.get();
        if left > 0 && !out.is_empty() {
            self.route_after.set(left - 1);
            self.route_cancel.set(left == 1);
        }
        if self.cancel.get() {
            return -1;
        }
        let Ok(at) = usize::try_from(at) else { return -1 };
        let Some(bytes) = self.bytes.get(at..) else { return -1 };
        let count = bytes.len().min(out.len());
        out[..count].copy_from_slice(&bytes[..count]);
        if let Some(byte) = self.corrupt_at.get().and_then(|bad| bad.checked_sub(at))
            .and_then(|offset| out[..count].get_mut(offset))
        {
            *byte ^= 0x20;
        }
        count as i32
    }
}

/// Menu, Contents, then "Middle Section" (c0.xhtml#mid). `$arm` runs once
/// Contents is open, so a fault or cancel lands in the jump, not the list.
macro_rules! jump_to_middle {
    ($owner:ident, $reader:ident, $source:ident, $ui:ident, $settings:ident, $ports:ident, $at:expr $(, $arm:block)?) => {{
        let at: u32 = $at;
        for (panel, at) in [(ReaderPanel::Menu, at), (ReaderPanel::Chapters, at + 1)] {
            if panel == ReaderPanel::Menu {
                $ui.panel = ReaderPanel::Menu;
            } else {
                $($arm)?
            }
            assert_eq!($ui.panel, panel);
            $ui.selected = 1;
            let handled = $owner.handle_ui_physical(
                &mut $reader, &mut $source, &mut $ui, $settings, Action::Activate, at, &mut $ports,
            );
            let _ = $owner.complete_ui_physical(&mut $reader, &mut $source, &mut $ui, handled, at, &mut $ports);
        }
    }};
}

/// Every book index file on the card, with its durable bytes.
fn index_files(card: &card::Card) -> Vec<(String, Vec<u8>)> {
    card.0.borrow().model.files.iter().filter(|(path, _)| path.contains("/Index/"))
        .map(|(path, file)| (path.clone(), file.durable.clone())).collect()
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
    synthetic_epub(None)
}

/// One chapter, or two with `second` paragraphs in the second.
fn synthetic_epub(second: Option<usize>) -> Vec<u8> {
    let two_chapters = second.is_some();
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
    for i in 0..second.unwrap_or(0) {
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
            let corrupt_at = core::cell::Cell::new(None);
            let route_after = core::cell::Cell::new(0u32);
            let route_cancel = core::cell::Cell::new(false);
            let cancel_auth = FlagCancel(&cancel_flag, &route_cancel);
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
            let mut raw = CancelSource {
                bytes: epub.clone(), cancel: &cancel_flag, cancel_after: &cancel_after, reads: &reads,
                route_after: &route_after, route_cancel: &route_cancel, corrupt_at: &corrupt_at,
            };
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

            // 2. A cancelled Contents jump to a fragment (#199). The cancel refuses the
            //    final freshness read, which says nothing about the book: the reader keeps
            //    its page and book index, and remembers no chapter failure.
            ports.cache_only = false;
            let index = index_files(&card);
            assert_eq!(index.len(), 1, "the open built one book index");
            // 2m. A cancelled Contents list load latches nothing: the reader keeps
            //     its page and the receipt waits for a reload.
            ui.panel = ReaderPanel::Menu;
            ui.selected = 1;
            cancel_after.set(2);
            let handled = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Activate, 1_900, &mut ports,
            );
            let _ = owner.complete_ui_physical(&mut reader, &mut source, &mut ui, handled, 1_900, &mut ports);
            assert!(cancel_flag.replace(false), "the Contents list load must be cancelled");
            assert!(owner.state.status() == Status::Ok && reader.opened() && reader.info() == page2,
                "a cancelled Contents list load keeps the reader and its page");
            assert_eq!(source.digest(), None, "a cancelled Contents list load closes the receipt");
            // 2l. Contents then reloads the receipt and builds its list from the book.
            ui.panel = ReaderPanel::Menu;
            ui.selected = 1;
            reads.set(0);
            let handled = owner.handle_ui_physical(
                &mut reader, &mut source, &mut ui, settings, Action::Activate, 1_950, &mut ports,
            );
            let _ = owner.complete_ui_physical(&mut reader, &mut source, &mut ui, handled, 1_950, &mut ports);
            assert!(ui.panel == ReaderPanel::Chapters && reader.chapter_menu().info().valid && reads.get() > 0,
                "Contents after a cancelled list load reloads the receipt and rebuilds the list");
            assert_eq!(source.digest(), Some(digest), "Contents reloaded the receipt under its digest");
            // Cancel mid-jump, after the first reads set deferred_fresh.
            jump_to_middle!(owner, reader, source, ui, settings, ports, 2_000, { cancel_after.set(2); });
            assert!(cancel_flag.get(), "mid-jump cancel must fire");
            cancel_flag.set(false);
            assert!(owner.state.opened() && reader.opened(), "a cancelled fragment jump keeps the reader open");
            assert_eq!(reader.info(), page2, "a cancelled fragment jump keeps the shown page");
            assert_eq!((ui.panel, ui.notice), (ReaderPanel::Chapters, ReaderUiNotice::ChapterJumpFailed),
                "a cancelled fragment jump stays on Contents with a failed-jump notice");
            assert_eq!(index_files(&card), index, "a cancelled fragment jump keeps the book index");
            assert_eq!(source.digest(), None, "a cancelled fragment jump closes the receipt");

            // 2s. A second cancel refuses the repaint's reload. It latches nothing and
            //     draws no ready frame; the receipt still waits for a reload.
            ui.panel = ReaderPanel::Page;
            ui.notice = ReaderUiNotice::None;
            cancel_flag.set(true);
            let painted = ports.paint_ui(&mut reader, &mut source, &mut owner, &mut ui, 2_300);
            cancel_flag.set(false);
            assert!(!painted.ready() && owner.state.status() == Status::Ok && reader.opened()
                && reader.info() == page2 && source.digest().is_none(),
                "a cancel during a repaint's reload latches nothing and keeps the reader");

            // 2v. A repaint whose source still waits on verification reloads first, so
            //     the check runs against the book's digest, not a closed source's.
            let mut checked = None;
            {
                let mut finish = |_: &mut dyn BookSource, expected: &Digest, _: &mut [u8], _: &dyn Fn() -> bool| {
                    checked = Some(*expected);
                    *expected == digest
                };
                // Another chapter, so the repaint must finish the check before it draws.
                let mut pending = source.with_verification_gate(page2.spine + 1, &mut finish, &|| true);
                let painted = ports.paint_ui(&mut reader, &mut pending, &mut owner, &mut ui, 2_400);
                assert!(painted.ready() && owner.state.opened() && reader.info() == page2,
                    "a pending-verification repaint after a cancel reloads first and draws the page");
            }
            assert_eq!(checked, Some(digest), "the pending check ran under the book's digest");
            assert_eq!(source.digest(), Some(digest), "the repaint reloaded the receipt under its digest");

            // 2f. A cancel that refuses the repaint's own reads latches nothing either.
            cancel_flag.set(true);
            let painted = ports.paint_ui(&mut reader, &mut source, &mut owner, &mut ui, 2_420);
            cancel_flag.set(false);
            assert!(!painted.ready() && owner.state.status() == Status::Ok && reader.opened()
                && reader.info() == page2 && source.digest().is_none(),
                "a cancelled repaint latches nothing and keeps the reader");
            // 2g. That cancel dropped the retained plan, so the next paint is the owner's
            //     page action, which reads only the Pages cache. Reload as the exit does,
            //     then cancel that action: it drew nothing, so the paint must not report
            //     a ready frame. (Its control pins that this paint takes the fallback.)
            assert!(source.reload_closed(digest), "the exit's reload restores the receipt");
            cancel_flag.set(true);
            let painted = ports.paint_ui(&mut reader, &mut source, &mut owner, &mut ui, 2_440);
            cancel_flag.set(false);
            assert!(!painted.ready() && owner.state.status() == Status::Ok && reader.opened()
                && reader.info() == page2,
                "a cancelled fallback paint never reports a ready frame");

            // 2p. A repaint reloads the receipt and draws the same page.
            let painted = ports.paint_ui(&mut reader, &mut source, &mut owner, &mut ui, 2_460);
            assert!(painted.ready() && owner.state.opened() && reader.info() == page2,
                "a repaint after a cancel reloads the receipt and draws the page");
            assert_eq!(source.digest(), Some(digest), "the repaint reloaded the receipt under its digest");

            // 2t. A turn reads and moves.
            for (action, at, page) in [(Action::Next, 2_500, page2.page + 1), (Action::Previous, 2_600, page2.page)] {
                let handled = owner.handle_ui_physical(&mut reader, &mut source, &mut ui, settings, action, at, &mut ports);
                let _ = owner.complete_ui_physical(&mut reader, &mut source, &mut ui, handled, at, &mut ports);
                assert_eq!((owner.state.status(), reader.info().page), (Status::Ok, page),
                    "a turn after a cancelled jump reloads the receipt and moves");
            }
            assert_eq!(reader.info(), page2);

            // 2r. The same jump then runs: the cancel was not kept as a chapter failure.
            jump_to_middle!(owner, reader, source, ui, settings, ports, 3_000, { reads.set(0); });
            let jump_reads = reads.get();
            let mid = reader.info();
            assert_eq!(ui.notice, ReaderUiNotice::None, "the jump after a cancelled one moves");
            assert!(mid.spine == page2.spine && mid.char_start > page2.char_start,
                "the jump after a cancelled one lands on the fragment, not page 2");

            // 2c. The same jump with only the route cancelled at its last read: the page
            //     publishes from reads the final check can't confirm, so the receipt closes.
            jump_to_middle!(owner, reader, source, ui, settings, ports, 5_000, { route_after.set(jump_reads); });
            assert!(route_cancel.get(), "the route cancel must fire on the jump's last read");
            route_cancel.set(false);
            assert_eq!(source.digest(), None, "a page published from unchecked reads closes the receipt");

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

            // 2b. The exit's reload rejects a wrong digest and succeeds for the expected one.
            let mut wrong = digest;
            wrong[0] ^= 0xff;
            assert!(!source.reload_closed(wrong), "reload_closed must fail closed on a wrong digest");
            assert_eq!(source.digest(), None);
            assert!(source.reload_closed(digest), "reload_closed must restore the matching stored receipt");
            assert_eq!(source.digest(), Some(digest));

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
                open: PhysicalOpen::Selected { digest, spine: mid.spine },
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
            // 2x. A receipt that no longer validates after a cancel is a changed
            //     book: the next action gives Stale and closes the reader.
            let mut ports = PhysicalOwner {
                storage: &mut storage,
                panel,
                backend: &mut parser,
                breaker: &mut breaker,
                cache: PhasedChapterCache { store: &mut pages, hash: &mut Hash, settings },
                open: PhysicalOpen::Loaded { digest, spine: page2.spine },
                cache_only: false,
            };
            let at_page2 = Progress { book, locator: Locator { spine: page2.spine, character: page2.char_start }, ..saved };
            assert!(owner.open_physical(&indexed, &mut reader, &mut source, book, at_page2,
                RestorePolicy::AllowInitialFallback, 6_000, &mut ports), "reopen: {:?}", owner.state.status());
            assert_eq!(reader.info(), page2, "the reopen lands on page 2");
            jump_to_middle!(owner, reader, source, ui, settings, ports, 6_100, { cancel_after.set(2); });
            assert!(cancel_flag.replace(false), "the jump after the reopen must be cancelled");
            assert!(reader.opened() && source.digest().is_none(), "the cancel left the receipt to reload");
            let receipt_path = card.0.borrow().model.files.keys()
                .find(|path| path.contains("/Imported/")).cloned().expect("the sealed receipt");
            let sealed = card.0.borrow().model.files[&receipt_path].clone();
            {
                let mut state = card.0.borrow_mut();
                let file = state.model.files.get_mut(&receipt_path).unwrap();
                for bytes in [&mut file.durable, &mut file.readable] {
                    bytes[0] ^= 0xff;
                }
            }
            ui.panel = ReaderPanel::Page;
            ui.notice = ReaderUiNotice::None;
            let handled = owner.handle_ui_physical(&mut reader, &mut source, &mut ui, settings, Action::Next, 6_200, &mut ports);
            let _ = owner.complete_ui_physical(&mut reader, &mut source, &mut ui, handled, 6_200, &mut ports);
            assert!(owner.state.status() == Status::Stale && !owner.state.opened() && !reader.opened(),
                "a changed receipt after a cancel gives Stale and closes the reader");
            assert!(!source.reload_closed(digest), "the changed receipt stays refused");
            drop(ports);
            card.0.borrow_mut().model.files.insert(receipt_path, sealed);
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
                Locator { spine: mid.spine, character: mid.char_start },
                "cancelled live exit must persist exact unsaved pre-turn locator"
            );
        })
        .unwrap();

    // A short cached chapter turns at once; a long one defers to Loading.
    // About 57 KB: many blocks, still under QUICK_VERIFY_BYTES.
    check_cross_chapter_cache_only(600, false);
    check_cross_chapter_cache_only(600, true);
    check_cross_chapter_cache_only(1_600, false);
    check_strict_cancel();

    println!("PASS cache_only cold-miss/hit and cancelled live exit save/reload");
    println!("PASS a cancelled Contents load or jump keeps the reader, its page and book index; the next repaint, turn, Contents and jump reload the receipt (or hash a strict source again) and run; a changed receipt after a cancel closes");
}

/// #199: the strict fallback opens when the receipt can't seal, so it has no
/// receipt to reload after a cancel. Its reload hashes the held book again.
fn check_strict_cancel() {
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
            let corrupt_at = core::cell::Cell::new(None);
            let route_after = core::cell::Cell::new(0u32);
            let route_cancel = core::cell::Cell::new(false);
            let cancel_auth = FlagCancel(&cancel_flag, &route_cancel);
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
                &mut receipt_state, &mut native_receipt, &route).unwrap();
            let mut rev_hash = ShaHasher::default();
            let mut rev_scratch = [0u8; BLOCK_BYTES];
            let mut raw = CancelSource {
                bytes: epub.clone(), cancel: &cancel_flag, cancel_after: &cancel_after, reads: &reads,
                route_after: &route_after, route_cancel: &route_cancel, corrupt_at: &corrupt_at,
            };
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
                &mut source, &digest, ports.storage, ports.panel, ports.backend, ports.cache.store,
                None, newt_phased_reader::index_build::Verification::Verified,
            ).unwrap();
            // The receipt's next write fails, so it can't seal and the open takes
            // the strict fallback. Later writes work again.
            let receipt_path = card.0.borrow().model.files.keys()
                .find(|path| path.contains("/Imported/")).cloned().expect("the receipt being imported");
            card.0.borrow_mut().io_fault = Some((receipt_path, model::Op::Write));
            let saved = Progress { book, ..Progress::default() };
            assert!(owner.open_physical(&indexed, &mut reader, &mut source, book, saved,
                RestorePolicy::AllowInitialFallback, 1_000, &mut ports),
                "the strict fallback opens: {:?}", owner.state.status());
            assert!(card.0.borrow().io_fault_seen, "the receipt write failed");
            card.0.borrow_mut().model.faults.clear();
            let page0 = reader.info();
            assert_eq!(source.digest(), Some(digest), "the strict fallback opened under the book's digest");

            jump_to_middle!(owner, reader, source, ui, settings, ports, 2_000, { cancel_after.set(2); });
            assert!(cancel_flag.replace(false), "the strict jump must be cancelled");
            assert!(owner.state.status() == Status::Ok && reader.opened() && reader.info() == page0
                && source.digest().is_none(),
                "a cancelled jump over the strict fallback keeps the reader and its page");
            ui.panel = ReaderPanel::Page;
            ui.notice = ReaderUiNotice::None;
            let handled = owner.handle_ui_physical(&mut reader, &mut source, &mut ui, settings, Action::Next, 2_100, &mut ports);
            let _ = owner.complete_ui_physical(&mut reader, &mut source, &mut ui, handled, 2_100, &mut ports);
            assert!(owner.state.status() == Status::Ok && reader.info().page == page0.page + 1
                && source.digest() == Some(digest),
                "a turn after a cancelled strict jump hashes the book again and moves");
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

fn check_cross_chapter_cache_only(paragraphs: usize, corrupt: bool) {
    let epub = synthetic_epub(Some(paragraphs));
    // About 100 stored bytes a paragraph.
    let long = paragraphs * 100 > newt_phased_reader::QUICK_VERIFY_BYTES as usize;
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
            let corrupt_at = core::cell::Cell::new(None);
            let route_after = core::cell::Cell::new(0u32);
            // Shared with the card, which can cancel the route at a faulted read.
            let route_cancel = std::rc::Rc::new(core::cell::Cell::new(false));
            let cancel_auth = FlagCancel(&cancel_flag, &route_cancel);
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
            let mut raw = CancelSource {
                bytes: epub.clone(), cancel: &cancel_flag, cancel_after: &cancel_after, reads: &reads,
                route_after: &route_after, route_cancel: &route_cancel, corrupt_at: &corrupt_at,
            };
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

            // A cache_only probe turns into a short cached chapter at once, and
            // defers a long one before verifying its range, until Loading shows.
            ports.cache_only = true;
            if corrupt {
                // The quick path still verifies the whole range: a corrupt
                // block mid-chapter, which publishing page 0 never reads,
                // keeps chapter 0 shown.
                let interior = epub.windows(29).position(|w| w == b"Second chapter paragraph 300:")
                    .expect("chapter 2 is stored");
                corrupt_at.set(Some(interior));
                let h = owner.handle_ui_physical(
                    &mut reader, &mut source, &mut ui, settings, Action::Next, 1_250, &mut ports,
                );
                let refused = owner.complete_ui_physical(
                    &mut reader, &mut source, &mut ui, h, 1_250, &mut ports,
                );
                assert_ne!(refused.work, ReaderUiWork::Intent(ReaderUiIntent::Repaint),
                    "a corrupt short chapter must not publish from the probe");
                assert_eq!(reader.info(), last_ch0, "a corrupt short chapter keeps chapter 0 shown");
            } else {
                reads.set(0);
                reader.reset_layout_pages();
                let h = owner.handle_ui_physical(
                    &mut reader, &mut source, &mut ui, settings, Action::Next, 1_300, &mut ports,
                );
                let probed = owner.complete_ui_physical(
                    &mut reader, &mut source, &mut ui, h, 1_300, &mut ports,
                );
                assert_eq!(reader.last_layout_pages(), 0, "a cache_only probe never lays out");
                if !long {
                    assert_eq!(probed.work, ReaderUiWork::Intent(ReaderUiIntent::Repaint),
                        "a short cached chapter must turn without deferring to Loading");
                    assert_eq!((reader.info().spine, reader.info().page), (1, 0));
                    assert!(reads.get() > 0, "the short chapter's range is verified before it publishes");
                } else {
                    assert_eq!(probed.work, ReaderUiWork::Intent(ReaderUiIntent::None),
                        "a long cross-chapter cache_only probe must defer until Loading is shown");
                    assert_eq!(reader.info(), last_ch0, "a deferred probe keeps chapter 0 shown");
                    let chapter_blocks = (paragraphs * 100 / BLOCK_BYTES) as u32;
                    assert!(reads.get() < chapter_blocks / 4,
                        "a deferred probe must not verify the long chapter's range ({} reads)", reads.get());

                    // After Loading is acknowledged (cache_only = false), the range IS
                    // verified before the cached page publishes (still 0 layout pages).
                    ports.cache_only = false;
                    reads.set(0);
                    let h = owner.handle_ui_physical(
                        &mut reader, &mut source, &mut ui, settings, Action::Next, 1_400, &mut ports,
                    );
                    let published = owner.complete_ui_physical(
                        &mut reader, &mut source, &mut ui, h, 1_400, &mut ports,
                    );
                    assert_eq!(published.work, ReaderUiWork::Intent(ReaderUiIntent::Repaint),
                        "cross-chapter retry after Loading must publish chapter 1");
                    assert_eq!((reader.info().spine, reader.info().page), (1, 0));
                    assert!(reads.get() >= chapter_blocks,
                        "cross-chapter retry must verify the target chapter range before publishing");
                    assert_eq!(reader.last_layout_pages(), 0, "verified cross-chapter retry must hit page cache");
                }
            }

            // A Contents jump back to chapter 1 reads the book index. A cancel that
            // lands on a failed index read keeps the index: the failure says nothing
            // about the file, and the route refuses the removal (#199). The same
            // failure without a cancel removes it, so the check can see a removal.
            if !long && !corrupt {
                ports.cache_only = false;
                let index = index_files(&card);
                assert_eq!(index.len(), 1, "the open built one book index");
                let here = reader.info();
                for cancelled in [true, false] {
                    jump_to_middle!(owner, reader, source, ui, settings, ports, if cancelled { 1_500 } else { 1_600 }, {
                        let mut state = card.0.borrow_mut();
                        state.io_fault = Some((index[0].0.clone(), model::Op::Read));
                        state.io_fault_seen = false;
                        state.fault_cancels = cancelled.then(|| route_cancel.clone());
                    });
                    let mut state = card.0.borrow_mut();
                    assert!(core::mem::take(&mut state.io_fault_seen), "the jump must read the book index");
                    state.fault_cancels = None;
                    drop(state);
                    if cancelled {
                        assert!(route_cancel.replace(false), "the faulted index read must cancel the route");
                        assert_eq!(index_files(&card), index, "a cancel at a failed index read keeps the book index");
                        assert!(owner.state.opened() && reader.opened() && reader.info() == here,
                            "a cancel at a failed index read keeps the reader and its page");
                    } else {
                        assert!(index_files(&card).is_empty(),
                            "a failed index read with no cancel removes the book index");
                    }
                }
            }

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
