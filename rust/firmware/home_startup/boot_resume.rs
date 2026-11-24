// SPDX-License-Identifier: MIT
//! Setup-owned NWBR authority from frozen reader_checkpoint.h:52–83 and
//! sleep_resume_store.h:15–27,102–111. No source or descriptor is owned here.
use newt_core::{
    durable::{Backend, Load},
    preferences::ViewSettings,
    progress::Progress,
    save_notice::{ReadingSaveNotice, ResumeIssue},
    sleep_intent::{SleepResumeIntent, SleepResumeStore, progress_checkpoint},
};

/// The one frozen SleepResumeTarget, discarded after setup. This is not a
/// reading-session checkpoint or a replacement for durable preferences.
pub struct Target {
    pub settings: ViewSettings,
    pub anchor: bool,
    pub issue: ResumeIssue,
    pub checked: bool,
    intent: SleepResumeIntent,
    loaded: Load<()>,
    intent_read: bool,
}
const _: () = assert!(core::mem::size_of::<Target>() <= 32);

fn read<B: Backend>(store: &mut B) -> (SleepResumeIntent, Load<()>) {
    match SleepResumeStore::new(store).load() {
        Load::Loaded(intent) => (intent, Load::Loaded(())),
        Load::Empty => (SleepResumeIntent::default(), Load::Empty),
        Load::Ambiguous => (SleepResumeIntent::default(), Load::Ambiguous),
        Load::Failed => (SleepResumeIntent::default(), Load::Failed),
    }
}

fn issue(loaded: Load<()>) -> ResumeIssue {
    if matches!(loaded, Load::Loaded(()) | Load::Empty) {
        ResumeIssue::None
    } else {
        ResumeIssue::Unavailable
    }
}

impl Target {
    /// This read precedes selection.restoreOnBoot. Only genuine power-on
    /// consults NWBR; an unavailable authority never names an older book.
    pub fn load<B: Backend>(
        store: &mut B,
        power_on: bool,
        settings: ViewSettings,
        notice: &mut ReadingSaveNotice,
    ) -> Self {
        let (intent, loaded) = if power_on {
            read(store)
        } else {
            (SleepResumeIntent::default(), Load::Empty)
        };
        if power_on {
            notice.set_resume(issue(loaded));
        }
        Self {
            settings,
            anchor: false,
            issue: ResumeIssue::None,
            checked: false,
            intent,
            loaded,
            intent_read: power_on,
        }
    }

    /// Pass this exact book to the existing selected-peer restore operation.
    pub fn committed_book(&self) -> u64 {
        if self.loaded == Load::Loaded(()) { self.intent.book } else { 0 }
    }

    /// Resolve the acknowledged peer even when the newest progress has the
    /// same CRC. The callback is the real selection.loadCheckpoint operation,
    /// on this same backend, and must retain its schema/extent validation.
    #[inline(never)]
    pub fn load_committed<B: Backend>(
        &mut self,
        store: &mut B,
        power_on: bool,
        active: bool,
        saved: &mut Progress,
        ready: bool,
        selection_verified: bool,
        load_checkpoint: impl FnOnce(&mut B, &mut Progress, u32) -> Load<()>,
    ) -> Option<&Self> {
        self.checked = false;
        self.issue = ResumeIssue::None;
        if !power_on || !active {
            return None;
        }
        let (intent, loaded) = if self.intent_read {
            (self.intent, self.loaded)
        } else {
            read(store)
        };
        self.checked = true;
        self.issue = issue(loaded);
        if !ready || !selection_verified || loaded != Load::Loaded(())
            || intent.book == 0 || intent.book != saved.book
        {
            return None;
        }
        let mut acknowledged = *saved;
        if load_checkpoint(store, &mut acknowledged, intent.checkpoint) != Load::Loaded(()) {
            return None;
        }
        if !intent.valid() || intent.book != acknowledged.book
            || intent.checkpoint != progress_checkpoint(&acknowledged)
        {
            return None;
        }
        // valid() above has checked the packed settings.
        self.settings = ViewSettings::unpacked(intent.settings)?;
        self.anchor = intent.settings & 8 != 0;
        *saved = acknowledged;
        Some(self)
    }
}

#[cfg(test)]
#[path = "boot_resume_tests.rs"]
mod tests;
