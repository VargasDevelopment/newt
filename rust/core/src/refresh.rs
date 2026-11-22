//! Independent shell and reader e-ink refresh budgets.
pub const FAST_PAINTS_BEFORE_FULL: u8 = 9;

#[derive(Clone, Copy, Debug, Default)]
pub struct UiRefresh {
    fast_paints: u8,
}

impl UiRefresh {
    pub fn needs_full(&self) -> bool {
        self.fast_paints >= FAST_PAINTS_BEFORE_FULL
    }
    pub fn painted(&mut self, full: bool) {
        self.fast_paints = if full {
            0
        } else {
            self.fast_paints
                .saturating_add(1)
                .min(FAST_PAINTS_BEFORE_FULL)
        };
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReadingRefresh {
    changes: u8,
    initial: bool,
    fast: UiRefresh,
}

impl Default for ReadingRefresh {
    fn default() -> Self {
        Self {
            changes: 0,
            initial: true,
            fast: UiRefresh::default(),
        }
    }
}

impl ReadingRefresh {
    /// Reset preserves the physical fast-paint budget across sessions.
    pub fn reset(&mut self) {
        self.changes = 0;
        self.initial = true;
    }
    pub fn page_changed(&mut self) {
        self.changes = self.changes.saturating_add(1);
    }
    pub fn needs_full(&self, interval: u32) -> bool {
        self.initial || u32::from(self.changes) >= interval || self.fast.needs_full()
    }
    pub fn painted(&mut self, full: bool) {
        if full {
            self.changes = 0;
            self.initial = false;
        }
        self.fast.painted(full);
    }
}
