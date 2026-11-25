// SPDX-License-Identifier: MIT

//! RTC resume evidence after Home's durable SD/NVS startup has completed.
//! Native code supplies initialized words and consumes the native magic;
//! neither this module nor the RTC record owns persistent preferences.
#[path = "retained_runtime/record.rs"]
pub mod record;
#[path = "retained_runtime/wake.rs"]
pub mod wake;

use newt_core::app::AppState;
use newt_core::preferences::ViewSettings;
use newt_core::progress::Progress;
use record::{BootReport, BootSnapshot, RestoreExpectation, SleepRecord};
use newt_c3_board::{Board, Boot, Error as BoardError, retained::RetainedError};
use newt_core::sd_resume::SdResumeRecord;

const _: () = assert!(newt_core::sd_resume::RETAINED_WORDS
    == newt_c3_board::retained_words::SD_RESUME_WORDS);

#[derive(Debug, PartialEq, Eq)]
pub enum SnapshotError {
    Regions(RetainedError),
    Boot(BoardError),
}

/// Read frozen's boot evidence before panel/input startup, consuming no magic.
/// The immutable snapshot is moved into the ordinary Rust startup callback.
///
/// # Safety
/// The full board::retained::with_retained exclusive Y-region contract and
/// Board::boot_before_acquisition initialized-SDK contract apply.
pub unsafe fn snapshot_before_board() -> Result<BootSnapshot, SnapshotError> {
    let snapshot = |words: newt_c3_board::retained_words::RetainedWords<'_>| {
        let record = words.read_sleep(|raw| SleepRecord::from_words(*raw));
        // SAFETY: same entry's live SDK; no record or output pointer is kept.
        let boot = unsafe { Board::boot_before_acquisition() }.map_err(SnapshotError::Boot)?;
        Ok(BootSnapshot {
            record,
            reset: boot.reset,
            cause: boot.wake,
            deep_wake: boot.deep_sleep_reset != 0,
            software_reset: boot.reset == Boot::SOFTWARE_RESET,
            gpio_wake: boot.wake == Boot::GPIO_WAKE,
        })
    };
    // SAFETY: caller grants the two sole retained owners for this short loan.
    unsafe { newt_c3_board::retained::with_retained(snapshot) }.map_err(SnapshotError::Regions)?
}

/// Home's published durable settings include the product defaults on missing
/// or corrupt storage. RTC settings are validated for resume compatibility,
/// but cannot replace that authority, even after an eligible warm wake.
/// When `expected.durable_present` is true, `*progress` holds the loaded
/// durable record and cannot be replaced by an older RTC sleep record.
/// Call before reader reopen and Quick Start selection; SD choices stay on
/// HomeClockSource and are never reconstructed from retained words.
pub fn restore_after_durable(
    snapshot: &BootSnapshot,
    consumed: &mut SleepRecord,
    state: &mut AppState,
    progress: Option<&mut Progress>,
    durable_settings: ViewSettings,
    expected: RestoreExpectation,
) -> BootReport {
    let mut resume_settings = durable_settings;
    record::restore_boot(
        snapshot,
        consumed,
        state,
        progress,
        Some(&mut resume_settings),
        expected,
    )
}

impl<L: core::fmt::Write, N: crate::nvs::Native, P: crate::home_startup::CardPort>
    crate::home_startup::HomeRuntime<'_, L, N, P>
{
    /// This is raw resume evidence; the existing record.valid() and source
    /// verification decide whether it may participate in selection restore.
    pub fn retained_sd_identity(&mut self) -> Result<SdResumeRecord, RetainedError> {
        self.bus.board.with_retained(|words| words.read_sd_resume(SdResumeRecord::from_words))
    }

    pub fn write_retained_sd_identity(&mut self, identity: &SdResumeRecord) -> Result<(), RetainedError> {
        self.bus.board.with_retained(|mut words| words.write_sd_resume(|out| identity.write_words(out)))
    }

    pub fn write_retained_sleep(&mut self, record: &SleepRecord) -> Result<(), RetainedError> {
        self.bus.board.with_retained(|mut words| words.write_sleep(|out| *out = record.words()))
    }

    /// Frozen setup consumes both first-word magics after restore, irrespective
    /// of eligibility. No checksum or remaining word is changed here.
    pub fn consume_retained_boot(&mut self) -> Result<(), RetainedError> {
        self.bus.board.with_retained(|mut words| {
            words.clear_sleep_magic();
            words.clear_sd_resume_magic();
        })
    }

    /// Uses this boot's durable reader settings without publishing RTC settings.
    /// Native magic clearing must follow even if no resume was eligible.
    pub fn restore_retained_boot(
        &mut self,
        snapshot: &BootSnapshot,
        consumed: &mut SleepRecord,
        state: &mut AppState,
        progress: Option<&mut Progress>,
        expected: RestoreExpectation,
    ) -> BootReport {
        restore_after_durable(
            snapshot,
            consumed,
            state,
            progress,
            self.preferences.settings(),
            expected,
        )
    }
}
