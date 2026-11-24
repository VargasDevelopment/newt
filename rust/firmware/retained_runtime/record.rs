//! RTC v7 retained sleep record policy.
//! MIT; see LICENSE. No RTC access, allocation, or durable authority.
#![forbid(unsafe_code)]

use newt_core::app::{AppState, Screen};
use newt_core::preferences::{Orientation, PACKED_SETTINGS, ViewSettings};
use newt_core::progress::{Locator, Progress};

pub const MAGIC: u32 = 0x4e57_5433;
pub const VERSION: u32 = 7;
pub const WORDS: usize = 17;
pub const BYTES: usize = 68;

/// Cold RTC remains primitive words until validation succeeds. The acquisition
/// boundary must supply initialized words: repr(C) never licenses reading
/// uninitialized Rust memory. Actual RTC acquisition belongs to native wiring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SleepRecord {
    pub magic: u32,
    pub version: u32,
    pub resume_screen: u32,
    pub book_low: u32,
    pub book_high: u32,
    pub spine: u32,
    pub character: u32,
    pub rejected_sleep: u32,
    pub reader_settings: u32,
    pub sequence: u32,
    pub active_low: u32,
    pub active_high: u32,
    pub forward_pages: u32,
    pub qualified_sessions: u32,
    pub checkpoint_saved: u32,
    pub recovery: u32,
    pub checksum: u32,
}
const _: () = assert!(core::mem::size_of::<SleepRecord>() == BYTES);
const _: () = assert!(core::mem::offset_of!(SleepRecord, rejected_sleep) == 28);
const _: () = assert!(core::mem::offset_of!(SleepRecord, checksum) == 64);

impl SleepRecord {
    pub const fn from_words(w: [u32; WORDS]) -> Self {
        Self {
            magic: w[0],
            version: w[1],
            resume_screen: w[2],
            book_low: w[3],
            book_high: w[4],
            spine: w[5],
            character: w[6],
            rejected_sleep: w[7],
            reader_settings: w[8],
            sequence: w[9],
            active_low: w[10],
            active_high: w[11],
            forward_pages: w[12],
            qualified_sessions: w[13],
            checkpoint_saved: w[14],
            recovery: w[15],
            checksum: w[16],
        }
    }
    pub const fn words(&self) -> [u32; WORDS] {
        [
            self.magic,
            self.version,
            self.resume_screen,
            self.book_low,
            self.book_high,
            self.spine,
            self.character,
            self.rejected_sleep,
            self.reader_settings,
            self.sequence,
            self.active_low,
            self.active_high,
            self.forward_pages,
            self.qualified_sessions,
            self.checkpoint_saved,
            self.recovery,
            self.checksum,
        ]
    }
    pub fn checksum(&self) -> u32 {
        let mut hash = 2_166_136_261u32;
        // Word-FNV, not byte-FNV; excludes magic and the checksum itself.
        for word in &self.words()[1..16] {
            hash = (hash ^ word).wrapping_mul(16_777_619);
        }
        hash
    }
    /// Bits 2/3 preserve uncertainty; low-bit value 3 is rejected, never cast.
    pub const fn recovery_kind(&self) -> Option<Recovery> {
        match self.recovery & 3 {
            0 => Some(Recovery::Ordinary),
            1 => Some(Recovery::PendingExact),
            2 => Some(Recovery::PendingAnchor),
            _ => None,
        }
    }
    /// Raw discriminator bit; does not authenticate a cold record.
    pub const fn is_sd(&self) -> bool {
        self.reader_settings & 4 != 0
    }
    /// The reader settings in the packed word; bits 2 and 3 are this record's own.
    pub fn reader_view(&self) -> Option<ViewSettings> {
        ViewSettings::unpacked(self.reader_settings)
    }
    /// The orientation wake diagnostics report for a reopened book: the
    /// durable target's when boot used it, else the one packed here.
    pub fn wake_orientation(&self, durable: Option<ViewSettings>) -> Option<Orientation> {
        durable.or_else(|| self.reader_view()).map(|settings| settings.orientation)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Valid,
    Missing,
    Version,
    Checksum,
    ScreenRange,
    BookIdentity,
    SettingsRange,
    EntryRange,
    Reserved,
}
impl Status {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Missing => "missing",
            Self::Version => "version",
            Self::Checksum => "checksum",
            Self::ScreenRange => "screen-range",
            Self::BookIdentity => "book-identity",
            Self::SettingsRange => "settings-range",
            Self::EntryRange => "sleep-entry-range",
            Self::Reserved => "reserved",
        }
    }
}
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boot {
    OtherReset,
    DeepSleepWake,
    AbortedSleepRestart,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    Ordinary,
    PendingExact,
    PendingAnchor,
}

pub const fn join_words(low: u32, high: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

#[inline(never)]
pub fn stamp_rejection(retained: &mut SleepRecord) {
    retained.rejected_sleep = 1;
    retained.checksum = retained.checksum();
}

pub fn classify_boot(deep_wake: bool, software_reset: bool, record: &SleepRecord) -> Boot {
    if deep_wake {
        return Boot::DeepSleepWake;
    }
    if software_reset
        && record.magic == MAGIC
        && record.version == VERSION
        && record.rejected_sleep == 1
        && record.checksum == record.checksum()
    {
        return Boot::AbortedSleepRestart;
    }
    Boot::OtherReset
}

/// A validated warm selection can own newer pending progress than the card.
/// Source bytes still require verification before any reader publication.
pub fn retained_selection(snapshot: &BootSnapshot, identity: &newt_core::sd_resume::SdResumeRecord) -> bool {
    identity.valid()
        && classify_boot(snapshot.deep_wake, snapshot.software_reset, &snapshot.record) != Boot::OtherReset
        && validate(&snapshot.record, identity.book_id(), true) == Status::Valid
}

pub fn make_record(
    state: &AppState,
    progress: Option<&Progress>,
    settings: ViewSettings,
    checkpoint_saved: bool,
    recovery: Recovery,
    sd_source: bool,
) -> SleepRecord {
    let destination = if state.screen == Screen::Sleeping {
        state.resume_screen
    } else {
        state.screen
    };
    let mut record = SleepRecord {
        version: VERSION,
        resume_screen: Screen::Home as u32,
        recovery: recovery as u32,
        ..SleepRecord::default()
    };
    if destination == Screen::EpubReading || recovery != Recovery::Ordinary {
        if let Some(progress) = progress.filter(|progress| progress.book != 0) {
            if destination == Screen::EpubReading {
                record.resume_screen = Screen::EpubReading as u32;
            }
            record.book_low = progress.book as u32;
            record.book_high = (progress.book >> 32) as u32;
            record.spine = progress.locator.spine;
            record.character = progress.locator.character;
            record.reader_settings = settings.packed() | (u32::from(sd_source) << 2);
            record.sequence = progress.sequence;
            record.active_low = progress.active_milliseconds as u32;
            record.active_high = (progress.active_milliseconds >> 32) as u32;
            record.forward_pages = progress.forward_pages;
            record.qualified_sessions = progress.qualified_sessions;
            record.checkpoint_saved = u32::from(checkpoint_saved);
        }
    }
    record.checksum = record.checksum();
    record.magic = MAGIC;
    record
}

pub fn validate(record: &SleepRecord, expected_book: u64, expected_sd_source: bool) -> Status {
    if record.magic != MAGIC {
        return Status::Missing;
    }
    if record.version != VERSION {
        return Status::Version;
    }
    if record.checksum != record.checksum() {
        return Status::Checksum;
    }
    if record.resume_screen != Screen::Home as u32
        && record.resume_screen != Screen::EpubReading as u32
    {
        return Status::ScreenRange;
    }
    if record.rejected_sleep > 1 || record.checkpoint_saved > 1 {
        return Status::EntryRange;
    }
    if record.recovery & !15 != 0 || record.recovery_kind().is_none() {
        return Status::Reserved;
    }
    if record.resume_screen == Screen::Home as u32
        && record.recovery_kind() == Some(Recovery::Ordinary)
    {
        if record.book_low != 0
            || record.book_high != 0
            || record.spine != 0
            || record.character != 0
            || record.reader_settings != 0
            || record.sequence != 0
            || record.active_low != 0
            || record.active_high != 0
            || record.forward_pages != 0
            || record.qualified_sessions != 0
            || record.checkpoint_saved != 0
        {
            return Status::Reserved;
        }
    } else {
        if expected_book == 0
            || join_words(record.book_low, record.book_high) != expected_book
            || record.is_sd() != expected_sd_source
        {
            return Status::BookIdentity;
        }
        // Bit 2 is the SD source; the rest are the shared packed settings.
        if record.reader_settings & !(PACKED_SETTINGS | 4) != 0
            || record.reader_view().is_none()
        {
            return Status::SettingsRange;
        }
    }
    Status::Valid
}

/// Every failure preserves all outputs; Home leaves optional book outputs alone.
pub fn restore(
    record: &SleepRecord,
    state: &mut AppState,
    progress: Option<&mut Progress>,
    settings: Option<&mut ViewSettings>,
    expected_book: u64,
    expected_sd_source: bool,
) -> bool {
    if validate(record, expected_book, expected_sd_source) != Status::Valid {
        return false;
    }
    let book = record.resume_screen == Screen::EpubReading as u32
        || record.recovery_kind() != Some(Recovery::Ordinary);
    if book {
        let (Some(progress), Some(settings), Some(view)) =
            (progress, settings, record.reader_view()) else {
            return false;
        };
        *progress = Progress {
            sequence: record.sequence,
            session_milliseconds: 0,
            book: join_words(record.book_low, record.book_high),
            locator: Locator {
                spine: record.spine,
                character: record.character,
            },
            active_milliseconds: join_words(record.active_low, record.active_high),
            forward_pages: record.forward_pages,
            qualified_sessions: record.qualified_sessions,
        };
        *settings = view;
    }
    let screen = if record.resume_screen == Screen::Home as u32 {
        Screen::Home
    } else {
        Screen::EpubReading
    };
    *state = AppState {
        screen,
        resume_screen: screen,
        ..AppState::default()
    };
    true
}

pub fn restore_on_boot(
    boot: Boot,
    record: &SleepRecord,
    state: &mut AppState,
    progress: Option<&mut Progress>,
    settings: Option<&mut ViewSettings>,
    expected_book: u64,
    expected_sd_source: bool,
) -> bool {
    boot != Boot::OtherReset
        && restore(
            record,
            state,
            progress,
            settings,
            expected_book,
            expected_sd_source,
        )
}

/// Caller supplies observations from its SDK boundary; this module reads no hardware.
#[derive(Clone, Copy, Debug)]
pub struct BootSnapshot {
    pub record: SleepRecord,
    pub reset: u32,
    pub cause: u32,
    pub deep_wake: bool,
    pub software_reset: bool,
    pub gpio_wake: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootReport {
    pub reset: u32,
    pub cause: u32,
    pub status: Status,
    pub requested: u32,
    pub restored: bool,
    pub checkpoint_saved: bool,
    pub wake: &'static str,
}
/// Portable restoreSleepBoot semantics, including unconditional one-shot consumption.
/// The caller publishes `wake` into DeviceInfo; logging and reset/cause acquisition are pending.
pub fn restore_boot(
    snapshot: &BootSnapshot,
    retained: &mut SleepRecord,
    state: &mut AppState,
    progress: Option<&mut Progress>,
    settings: Option<&mut ViewSettings>,
    expected_book: u64,
    expected_sd_source: bool,
) -> BootReport {
    let eligibility = classify_boot(
        snapshot.deep_wake,
        snapshot.software_reset,
        &snapshot.record,
    );
    let status = validate(&snapshot.record, expected_book, expected_sd_source);
    let restored = restore_on_boot(
        eligibility,
        &snapshot.record,
        state,
        progress,
        settings,
        expected_book,
        expected_sd_source,
    );
    let wake = if restored {
        if eligibility == Boot::AbortedSleepRestart {
            "sleep entry rejected / resumed"
        } else if snapshot.gpio_wake {
            "power-button wake"
        } else {
            "deep-sleep wake"
        }
    } else if eligibility == Boot::OtherReset {
        "cold / reset boot"
    } else {
        "wake state unavailable"
    };
    retained.magic = 0;
    BootReport {
        reset: snapshot.reset,
        cause: snapshot.cause,
        status,
        requested: snapshot.record.resume_screen,
        restored,
        checkpoint_saved: status == Status::Valid && snapshot.record.checkpoint_saved == 1,
        wake,
    }
}
