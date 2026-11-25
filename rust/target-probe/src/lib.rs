//! Linkable RISC-V core probe, NOT an X3 firmware or hardware driver.
//! This monomorphizes generic durable operations on the target rather than
//! merely emitting an rlib whose generic bodies wait for a future caller.
#![no_std]
mod admission_probe;
mod book_graph_probe;
mod catalog_probe;
mod clock_probe;
mod identity_probe;
mod persistence_probe;
mod sleep_crossink_probe;
mod stats_probe;
mod tracker_probe;
use newt_core::{
    durable::*, preferences, preferences_store, progress, progress_store, refresh, sd_record,
};

struct Memory<const N: usize> {
    slots: [[u8; N]; 2],
    present: [bool; 2],
    fault: u32,
}
impl<const N: usize> Backend for Memory<N> {
    fn begin(&mut self, _: &str, _: bool) -> StorageOpen {
        StorageOpen::Opened
    }
    fn end(&mut self) {}
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        let index = usize::from(key == "slot1");
        if self.fault & 1 != 0 {
            return BlobRead::Failed;
        }
        if !self.present[index] {
            return BlobRead::Missing;
        }
        let count = N.min(output.len());
        output[..count].copy_from_slice(&self.slots[index][..count]);
        BlobRead::Present(N)
    }
    fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        let index = usize::from(key == "slot1");
        if bytes.len() != N {
            return 0;
        }
        self.slots[index].copy_from_slice(bytes);
        self.present[index] = true;
        if self.fault & 2 != 0 { 0 } else { bytes.len() }
    }
    fn readback_durable(&self) -> bool {
        self.fault & 4 == 0
    }
}

/// Numeric inputs keep the generic bodies live through target optimization.
/// The probe's failure backend is synthetic, not an implementation of SD I/O.
#[unsafe(no_mangle)]
pub extern "C" fn newt_rust_core_probe(input: u32) -> u32 {
    let mut memory = Memory {
        slots: [[0; 20]; 2],
        present: [false; 2],
        fault: input >> 8,
    };
    let mut records = RecordStore::<_, TimeZoneCodec, 20>::new(&mut memory);
    let saved = records.save(input % 15) == Commit::Saved;
    let loaded = matches!(records.load(), Load::Loaded(_));
    let matched = matches!(
        records.load_matching(|v| v == input % 15, |a, b| a == b),
        Load::Loaded(_)
    );
    let protected = records.save_protected(input.wrapping_add(1) % 15, input % 15) == Commit::Saved;
    let mut quiet = || input & 1 != 0;
    let mut admission = Admission {
        quiet: Some(&mut quiet),
        entered: false,
        deferred: false,
    };
    let admitted = records.save_admitted(input % 15, &mut admission) == Commit::Saved;
    let strict = matches!(
        RecordStore::<_, TimeZoneCodec, 20, false>::new(&mut memory).load(),
        Load::Loaded(_)
    );
    let mut published = restore::<_, TimeZoneCodec, 20>(&mut memory, 0);
    let committed = commit_publish::<_, TimeZoneCodec, 20>(&mut memory, input % 15, &mut published)
        == Commit::Saved;
    let mut bytes = [0; 20];
    ClockFormatCodec::encode(input % 2, input, &mut bytes);
    let clock = ClockFormatCodec::decode(&bytes).unwrap_or(0);
    SleepScreenCodec::encode(input % 8, input, &mut bytes);
    let sleep = SleepScreenCodec::decode(&bytes).unwrap_or(0);
    let prefs = preferences::Preferences::default()
        .encode()
        .unwrap_or([0; 24]);
    let settings = preferences::Preferences::decode(&prefs)
        .unwrap_or_default()
        .settings;
    let geometry = preferences::geometry(settings);
    let value = progress::Progress {
        sequence: input,
        book: u64::from(input) + 1,
        ..Default::default()
    };
    let page = value.encode().unwrap_or([0; 52]);
    let planned = progress::plan(&page, &[], value).map_or(0, |p| p.value.sequence);
    let mut progress_memory = Memory {
        slots: [[0; 52]; 2],
        present: [false; 2],
        fault: input >> 8,
    };
    let mut store = progress_store::ProgressStore::new(&mut progress_memory);
    let mut live = value;
    let mut peers = progress_store::PeerLocators::default();
    let progress_saved =
        store.save_with_peers(&mut live, &mut peers, None, input & 1 != 0) == Commit::Saved;
    let _ = store.load();
    let _ = store.load_checkpoint(input);
    let _ = store.load_peer_locators(&mut peers);
    let _ = store.save(&mut live, Some(input), false);
    let mut preferences_memory = Memory {
        slots: [[0; 24]; 2],
        present: [false; 2],
        fault: input >> 8,
    };
    let mut preferences_store = preferences_store::PreferencesStore::new(&mut preferences_memory);
    let mut preferences_value = preferences::Preferences::default();
    preferences_value.settings.orientation = if input & 1 != 0 {
        preferences::Orientation::Portrait
    } else {
        preferences::Orientation::LandscapeCcw
    };
    preferences_value.settings.slim_progress_bar = input & 2 != 0;
    preferences_value.settings.refresh_pages = [1, 5, 10][(input % 3) as usize];
    let preferences_saved = preferences_store.save(&mut preferences_value).is_ok();
    let preferences_loaded = preferences_store.load().is_ok();
    let mut record = [0; 256];
    sd_record::header(&mut record, b"newt_reader");
    sd_record::seal(&mut record, b"NRS2", b"slot0", 20, input);
    let intact = sd_record::intact(&record, b"NRS2");
    let mut refresh = refresh::ReadingRefresh::default();
    refresh.painted(input & 1 != 0);
    refresh.page_changed();
    refresh.reset();
    u32::from(saved)
        | u32::from(loaded) << 1
        | u32::from(matched) << 2
        | u32::from(protected) << 3
        | u32::from(admitted) << 4
        | u32::from(strict) << 5
        | u32::from(committed) << 6
        | u32::from(intact) << 7
        | u32::from(refresh.needs_full(input)) << 8
        | (clock
            ^ sleep
            ^ published
            ^ planned
            ^ u32::from(geometry.width)
            ^ u32::from(progress_saved)
            ^ u32::from(preferences_saved)
            ^ u32::from(preferences_loaded))
            << 9
            ^ identity_probe::probe(input).rotate_left(17)
}

/// Target code-generation probe; caller lends existing buffers, no frame owner.
#[unsafe(no_mangle)]
pub fn newt_rust_frame_probe(
    logical: &mut newt_core::frame::Frame,
    native: &mut newt_core::frame::Frame,
    input: u32,
) -> u32 {
    use newt_core::frame::*;
    let orientation = if input & 1 == 0 {
        preferences::Orientation::Portrait
    } else {
        preferences::Orientation::LandscapeCcw
    };
    match input % 7 {
        0 => {
            to_native_frame(logical, native, orientation);
            1
        }
        1 => {
            to_native_selectors(logical, native, orientation);
            1
        }
        2 => u32::from(restore_native_frame_prefix(
            logical,
            native,
            input as usize,
            orientation,
        )),
        3 => u32::from(to_native_selector_regions(
            logical,
            native,
            &[NativeRegion {
                x: (input % 100) as u16 * 8,
                y: 0,
                width: 8,
                height: 1,
            }],
        )),
        4 => u32::from(fill_native_rect(native, (input % 529) as i32, 0, 1, 1)),
        5 => u32::from(publish_staged_frame(logical, native, |stage| {
            stage[0] ^= input as u8;
            input & 2 == 0
        })),
        _ => u32::from(
            publish_with_action_storage(logical, native, |stage, action| {
                stage[0] ^= input as u8;
                action[1] = input as u8;
                input & 2 == 0
            }) == ActionFrameResult::Published,
        ),
    }
}

/// Valid UTC calendar input; formatting uses nine stack bytes.
#[unsafe(no_mangle)]
pub fn newt_rust_clock_probe(input: u32) -> u32 {
    use newt_core::time_zone::*;
    let value = local_clock_time(
        ClockTime {
            year: 2000 + (input % 200) as u16,
            month: 1 + (input % 12) as u8,
            day: 1 + (input % 28) as u8,
            hour: (input % 24) as u8,
            minute: (input % 60) as u8,
        },
        TimeZone::from_raw_or_utc(input as u8),
    );
    u32::from(value.year)
        ^ u32::from(value.month) << 16
        ^ u32::from(value.day) << 20
        ^ u32::from(value.hour) << 25
        ^ u32::from(value.minute)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

mod external_progress_probe;
pub use external_progress_probe::ProbeBackend as ExternalProgressProbeBackend;
#[unsafe(no_mangle)]
pub fn newt_rust_external_progress_probe(
    backend: &mut ExternalProgressProbeBackend<'_>,
    digest: Option<&[u8; 32]>,
    value: &mut newt_core::progress::Progress,
    peers: &mut newt_core::progress_store::PeerLocators,
    checkpoint: Option<u32>,
    strict: bool,
    admission: &mut newt_core::durable::Admission<'_>,
    operation: u8,
) -> (u8, newt_core::external_progress_store::Status) {
    external_progress_probe::runtime_probe(
        backend, digest, value, peers, checkpoint, strict, admission, operation,
    )
}

mod stats_service_probe;

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_stats_service_probe(
    b: &mut stats_service_probe::Borrowed<'_>,
    op: u8,
    open: &newt_core::reading_stats_service::StatsBook,
    clock: newt_core::stats_clock::StatsClock,
    mark: &newt_core::tracker::ReadingDayMark,
    cancel: &mut dyn FnMut() -> bool,
    sha: newt_core::reading_stats_once::Sha256<'_>,
    summary: &mut newt_core::reading_stats::StatsSummary,
    facts: &mut newt_core::reading_stats::BookFacts,
    extras: &mut newt_core::reading_stats_service::SleepExtras,
    unsaved: u32,
) -> newt_core::reading_stats_service::StatsReport {
    stats_service_probe::probe(
        b, op, open, clock, mark, cancel, sha, summary, facts, extras, unsaved,
    )
}

mod zip_window_probe;

mod inflate_probe;

mod zip_container_borrowed;

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_zip_container_catalog_probe(
    input: &[u8],
    names: &mut [u8],
    entries: &mut [newt_core::zip_container::Entry],
    scan: &mut [u8; 4099],
    seq: &mut [u8; 1024],
    path: &[u8],
) -> (newt_core::zip_container::Status, usize, bool) {
    zip_container_borrowed::catalog(input, names, entries, scan, seq, path)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_zip_container_reader_probe(
    input: &[u8],
    entry: &newt_core::member_check::Checked,
    state: &mut newt_core::inflate::State,
    dictionary: &mut [u8; 32768],
    staging: &mut [u8; 2048],
    output: &mut [u8],
) -> (newt_core::zip_container::Status, i32, u32) {
    zip_container_borrowed::stream(input, entry, state, dictionary, staging, output)
}

mod css_borrowed;
mod css_probe;

mod xml_input_probe;

mod hyphenator_probe;

mod font_layout_probe;

mod image_probe_borrowed;
mod image_probe_export;

mod xml_session_probe;

mod package_parsers_probe;

pub mod page_renderer_probe;

mod png_probe;

mod app_probe;
mod chapter_probe;

pub mod ft_font_probe;

mod jpeg_probe;

mod reader_atlas_borrowed;

mod epub_admission_borrowed;

mod reader_navigation_borrowed;

mod ui_canvas_borrowed;

mod system_ui_borrowed;
mod stage_home_borrowed;

mod library_ui_borrowed;

mod device_ui_borrowed;
mod home_clock_borrowed;

#[cfg(feature = "dev-usb")]
mod wake_evidence_borrowed;
#[cfg(feature = "tiny-app")]
mod tiny_app_borrowed;
