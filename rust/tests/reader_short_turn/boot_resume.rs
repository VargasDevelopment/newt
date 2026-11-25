// SPDX-License-Identifier: MIT
//! Boot's restore decision handed to the real reader open (#107). A page start
//! saved under other settings (a text size or an orientation) must reopen at
//! the page that holds its locator; one saved under the reopen settings stays
//! an exact page.
use super::*;
use newt_core::{app::{AppState, Screen}, durable::{Commit, Load},
    external_progress_store::ExternalProgressStore, preferences::{Orientation, TextSize}, progress::Locator,
    save_notice::ReadingSaveNotice, sleep_intent::{SleepResumeIntent, SleepResumeStore, progress_checkpoint}};
use newt_native_io::UiIo;
use newt_native_storage::RouteState;
use newt_phased_reader::PhasedChapterCache;
use reader_restore_policy::{DurableCheckpoint, boot_restore_policy};
use retained_record::{BootSnapshot, Recovery, RestoreExpectation, make_record, restore_boot};
use upgrade::{with_pages, with_router};

/// The page start the real opener shows for `saved` under `settings`, or None when it refuses.
fn open(panel: &mut newt_c3_board::ReaderPanel<'_, '_>, io: &UiIo, card: &mut card::Card, bytes: &[u8],
    settings: ViewSettings, saved: Progress, policy: RestorePolicy,
) -> Option<u32> {
    let mut raw = Source(bytes.to_vec());
    let mut digest = [0; 32];
    assert!(hash(&raw.0, &mut digest));
    let mut bank = [0; BANK_BYTES];
    let mut storage = Storage::new(&mut bank);
    let mut parser = newt_target_expat::Expat::default();
    let mut breaker = linebreak::NativeBreak;
    let route = RouteState::new(reader_book::CACHE_ROOT,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
    let route = route.lend(&Never, now);
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [Run::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), settings.page_layout());
    let mut authorize = |raw: &mut dyn BookSource, expected: &[u8; 32], _: &mut [u8], allowed: &dyn Fn() -> bool| {
        let mut bytes = vec![0; raw.size() as usize];
        let mut actual = [0; 32];
        allowed() && raw.read_at(0, &mut bytes) == bytes.len() as i32 && hash(&bytes, &mut actual) && &actual == expected
    };
    let mut source = StrictSource::new(&mut raw, &digest).with_reopen_authorizer(&mut authorize, &|| true);
    let mut shown = None;
    with_pages(card, io, &route, |pages| {
        let mut owner = ExternalOwner::default();
        let mut ports = newt_reader_ui::PhysicalOwner { storage: &mut storage, panel, backend: &mut parser,
            breaker: &mut breaker, cache: PhasedChapterCache { store: pages, hash: &mut Hash, settings },
            open: PhysicalOpen::Import, cache_only: false };
        if owner.open_physical(&mut reader, &mut source, saved.book, saved, policy, now(), &mut ports) {
            shown = Some(reader.info().char_start);
        }
        reader.close();
    });
    shown
}

pub fn check(bytes: Vec<u8>) {
    let mut tokens = [0u8; 2];
    // SAFETY: the existing host Board fake treats these as disjoint tokens.
    let (mut board, pending, _) = unsafe { newt_c3_board::Board::acquire_x3_optional_sd(
        (&mut tokens[0] as *mut u8).cast(), (&mut tokens[1] as *mut u8).cast()) }.unwrap();
    drop(pending);
    board.with_reader_io(|panel, io| {
        let portrait = ViewSettings { orientation: Orientation::Portrait, ..ViewSettings::default() };
        // Text size first, so the compiled policy controls fail on its rows (#41).
        let medium = ViewSettings { text_size: TextSize::Px27, ..portrait };
        pair(panel, io, &bytes, medium, ViewSettings { text_size: TextSize::Px24, ..portrait }, "27 -> 24 px");
        pair(panel, io, &bytes, portrait, ViewSettings::default(), "Portrait -> Landscape");
    }).unwrap();
    println!("PASS boot restore policy reopens sleep and power-on pages under the durable settings");
}

/// A page saved under `under` reopens as that exact page under `under`, and as
/// the page holding it under `other` or the defaults.
fn pair(panel: &mut newt_c3_board::ReaderPanel<'_, '_>, io: &UiIo, bytes: &[u8], under: ViewSettings,
    other: ViewSettings, what: &str,
) {
    let mut digest = [0; 32];
    assert!(hash(bytes, &mut digest));
    let book = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let at = |character| Progress { book, locator: Locator { spine: 0, character }, ..Progress::default() };
    let mut card = card::Card::default();
    // A page start under `under` that is not one under `other`.
    let start = (1..40).find_map(|page| {
        let start = open(panel, io, &mut card, bytes, under, at(page * 300), RestorePolicy::Validated)?;
        let holding = open(panel, io, &mut card, bytes, other, at(start), RestorePolicy::Validated)?;
        (start != holding).then_some(start)
    }).unwrap_or_else(|| panic!("{what}: page boundaries differ"));
    assert_eq!(open(panel, io, &mut card, bytes, other, at(start), RestorePolicy::ExactPage), None,
        "{what}: the saved page start is not a page start under the other settings");
    // The page each durable setting must reopen.
    let defaults = ViewSettings::default();
    let [other_page, default_page] = [other, defaults].map(|settings| if settings == under { Some(start) } else {
        open(panel, io, &mut card, bytes, settings, at(start), RestorePolicy::Validated)
    });

    // Warm wake: the sleep record was saved under `under`. The durable
    // settings match, differ (a lost save acknowledgement) or are the
    // defaults (missing preferences).
    let state = AppState { screen: Screen::EpubReading, ..AppState::default() };
    for (case, settings, page) in [("same", under, Some(start)), ("lost-ack", other, other_page),
        ("prefs-missing", defaults, default_page)] {
        for recovery in [Recovery::Ordinary, Recovery::PendingExact] {
            let record = make_record(&state, Some(&at(start)), under, true, recovery, true);
            let policy = boot_restore_policy(None, Some(&record), settings);
            assert_eq!(open(panel, io, &mut card, bytes, settings, at(start), policy), page,
                "warm {case}: a sleep page start reopens under the durable settings ({what}, {recovery:?}, {policy:?})");
        }
    }

    // Power-on: the resume target names a page start saved under `under`.
    // Everything below is read back from the card's durable bytes, as a cold
    // boot does.
    for (case, durable, page) in [("same", under, Some(start)), ("stale", other, other_page)] {
        let mut saved = at(start);
        with_router(&mut card, io, |store| {
            assert_eq!(ExternalProgressStore::new(store, Some(&digest)).save(&mut saved, None, None, false, None),
                Commit::Saved);
            let intent = SleepResumeIntent { book, checkpoint: progress_checkpoint(&saved),
                settings: under.packed() | 4 };
            assert_eq!(SleepResumeStore::new(store).save(intent), Commit::Saved);
        });
        card = card.cold_bytes();
        let (settings, policy, progress) = with_router(&mut card, io, |store| {
            let mut target = home_startup::boot_resume::Target::load(store, true, durable, &mut ReadingSaveNotice::default());
            let mut progress = Progress::default();
            let ready = ExternalProgressStore::new(store, Some(&digest)).load(&mut progress) == Load::Loaded(());
            let target = target.load_committed(store, true, true, &mut progress, ready, true,
                |store, out, at| ExternalProgressStore::new(store, Some(&digest)).load_checkpoint(out, at))
                .expect("committed resume target");
            (target.settings, boot_restore_policy(Some(DurableCheckpoint::of(target)), None, target.settings), progress)
        });
        assert_eq!(settings, durable, "power-on {case}: durable settings win ({what})");
        assert_eq!(open(panel, io, &mut card, bytes, settings, progress, policy), page,
            "power-on {case}: the resume target reopens under the durable settings ({what}, {policy:?})");
    }

    // #111: warm wake with a stale RTC sleep record (saved under `other`) while
    // durable storage holds a newer sequence saved under `under` must keep the
    // newer durable progress and reopen at the page holding it under `other`.
    let older_rtc = Progress { sequence: 2, ..at(0) };
    let newer_durable = Progress { sequence: 3, ..at(start) };
    let stale_record = make_record(&state, Some(&older_rtc), other, true, Recovery::Ordinary, true);
    let snapshot = BootSnapshot { record: stale_record, reset: 8, cause: 7,
        deep_wake: true, software_reset: false, gpio_wake: true };
    let mut restored_progress = newer_durable;
    let mut consumed = stale_record;
    let mut restored_app = AppState::default();
    let mut ignored = other;
    let report = restore_boot(&snapshot, &mut consumed, &mut restored_app,
        Some(&mut restored_progress), Some(&mut ignored),
        RestoreExpectation { book, sd_source: true, durable_present: true });
    assert!(report.restored && report.durable_progress && report.checkpoint_saved,
        "warm stale-RTC: newer durable progress supersedes stale RTC ({what})");
    assert_eq!(restored_progress, newer_durable,
        "warm stale-RTC: newer durable progress is kept ({what})");
    let policy = boot_restore_policy(Some(DurableCheckpoint::PageStart), Some(&stale_record), other);
    assert_eq!(open(panel, io, &mut card, bytes, other, restored_progress, policy), other_page,
        "warm stale-RTC: newer durable progress reopens under durable settings ({what}, {policy:?})");
}
