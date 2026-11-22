//! Awake-only wake observations for diagnostics. MIT.
#![no_std]
#![forbid(unsafe_code)]
use core::fmt::Write;
use newt_core::app::{Action, Screen};
pub const UNAVAILABLE: u8 = 255;
pub const LINES: u32 = 13;
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Attempt {
    #[default]
    NotRequested,
    Pending,
    LargestBlockGuard,
    FreeHeapGuard,
    OpenFailed,
    Succeeded,
    SourceUnavailable,
}
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum InputSource {
    #[default]
    Unavailable,
    Physical,
    Usb,
}
#[derive(Clone, Copy)]
pub struct Input {
    pub milliseconds: u32,
    pub source: InputSource,
    pub button: u8,
    pub action: u8,
    pub before: u8,
    pub after: u8,
    pub reader_open: bool,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            milliseconds: 0,
            source: InputSource::Unavailable,
            button: UNAVAILABLE,
            action: UNAVAILABLE,
            before: UNAVAILABLE,
            after: UNAVAILABLE,
            reader_open: false,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Evidence {
    pub book: u64,
    pub reset: u32,
    pub cause: u32,
    pub spine: u32,
    pub character: u32,
    pub actual_spine: u32,
    pub actual_character: u32,
    pub first_generation: u32,
    pub first_physical: Input,
    pub first_reading_home: Input,
    pub available: bool,
    pub eligible: bool,
    pub record: u8,
    pub requested: u8,
    pub restored_screen: u8,
    pub rejected_entry: u8,
    pub saved_orientation: u8,
    pub tried_orientation: u8,
    pub reader_status: u8,
    pub restore_failure: u8,
    pub restore_policy: u8,
    pub attempt: Attempt,
    pub first_screen: u8,
    pub restored: bool,
    pub checkpoint_saved: bool,
    pub reader_open: bool,
    pub durable_resume: bool,
}
impl Default for Evidence {
    fn default() -> Self {
        Self {
            book: 0,
            reset: 0,
            cause: 0,
            spine: 0,
            character: 0,
            actual_spine: 0,
            actual_character: 0,
            first_generation: 0,
            first_physical: Input::default(),
            first_reading_home: Input::default(),
            available: false,
            eligible: false,
            record: UNAVAILABLE,
            requested: UNAVAILABLE,
            restored_screen: UNAVAILABLE,
            rejected_entry: UNAVAILABLE,
            saved_orientation: UNAVAILABLE,
            tried_orientation: UNAVAILABLE,
            reader_status: UNAVAILABLE,
            restore_failure: UNAVAILABLE,
            restore_policy: UNAVAILABLE,
            attempt: Attempt::NotRequested,
            first_screen: UNAVAILABLE,
            restored: false,
            checkpoint_saved: false,
            reader_open: false,
            durable_resume: false,
        }
    }
}
/// A short observation of the real reader, without retaining its source loan.
pub trait ReaderObservation {
    fn status(&self) -> u8;
    fn restore_failure(&self) -> u8;
    fn orientation(&self) -> u8;
    fn opened(&self) -> bool;
    fn page(&self) -> (u32, u32);
}
impl Evidence {
    pub fn initial_reader_attempt(
        &mut self,
        outcome: Attempt,
        reader: &impl ReaderObservation,
        policy: u8,
    ) {
        if !self.available || self.attempt != Attempt::Pending {
            return;
        }
        self.attempt = outcome;
        self.restore_policy = policy;
        self.reader_status = reader.status();
        self.restore_failure = reader.restore_failure();
        self.tried_orientation = reader.orientation();
        self.reader_open = reader.opened();
        if self.reader_open {
            (self.actual_spine, self.actual_character) = reader.page();
        }
    }
    pub fn completed_paint(&mut self, screen: Screen, generation: u32) {
        if !self.available || self.first_screen != UNAVAILABLE || generation == 0 {
            return;
        }
        self.first_screen = screen as u8;
        self.first_generation = generation;
    }
    pub fn consumed_input(
        &mut self,
        source: InputSource,
        button: u8,
        action: Action,
        now: u32,
        before: Screen,
        after: Screen,
        reader_was_open: bool,
    ) {
        if !self.available || source == InputSource::Unavailable {
            return;
        }
        let event = Input {
            milliseconds: now,
            source,
            button,
            action: action as u8,
            before: before as u8,
            after: after as u8,
            reader_open: reader_was_open,
        };
        if source == InputSource::Physical && self.first_physical.source == InputSource::Unavailable
        {
            self.first_physical = event;
        }
        if before == Screen::EpubReading
            && after == Screen::Home
            && self.first_reading_home.source == InputSource::Unavailable
        {
            self.first_reading_home = event;
        }
    }
}
fn word(v: bool) -> &'static str {
    if v { "yes" } else { "no" }
}
fn screen(v: u8) -> &'static str {
    match v {
        0 => "Home",
        1 => "Sample",
        2 => "Settings",
        3 => "Sleep",
        4 => "Reading",
        5 => "App",
        6 => "Library",
        _ => "unavailable",
    }
}
fn action(v: u8) -> &'static str {
    match v {
        0 => "Prev",
        1 => "Next",
        2 => "Select",
        3 => "Back",
        4 => "Sleep",
        5 => "Read",
        _ => "?",
    }
}
fn orientation(v: u8) -> &'static str {
    match v {
        0 => "portrait",
        1 => "CCW",
        _ => "unavailable",
    }
}
fn attempt(v: Attempt) -> &'static str {
    match v {
        Attempt::NotRequested => "not requested",
        Attempt::Pending => "not completed",
        Attempt::LargestBlockGuard => "largest-block guard",
        Attempt::FreeHeapGuard => "free-heap guard",
        Attempt::OpenFailed => "failed",
        Attempt::Succeeded => "succeeded",
        Attempt::SourceUnavailable => "SD unavailable",
    }
}
/// Write one ASCII diagnostic line through the caller's bounded writer.
pub fn write_line(e: &Evidence, line: u32, b: &mut impl Write) {
    let p = &e.first_physical;
    let h = &e.first_reading_home;
    match line {
        0 => {
            let _ = write!(
                b,
                "Reset {} / wake {} / eligible {}",
                e.reset,
                e.cause,
                word(e.eligible)
            );
        }
        1 => {
            let names = [
                "valid", "missing", "version", "checksum", "route", "book", "settings", "entry",
                "reserved",
            ];
            let _ = write!(
                b,
                "Record {} / rejected {} / save {}",
                names.get(usize::from(e.record)).copied().unwrap_or("?"),
                if e.rejected_entry == UNAVAILABLE {
                    "?"
                } else {
                    word(e.rejected_entry == 1)
                },
                if e.record == 0 || e.durable_resume {
                    word(e.checkpoint_saved)
                } else {
                    "?"
                }
            );
        }
        2 => {
            let _ = write!(
                b,
                "{} -> {} / restored {}",
                if e.durable_resume {
                    "Saved book"
                } else if e.requested == UNAVAILABLE {
                    "untrusted"
                } else {
                    screen(e.requested)
                },
                screen(e.restored_screen),
                word(e.restored)
            );
        }
        3 => {
            let _ = write!(
                b,
                "Open {} / {}",
                attempt(e.attempt),
                match e.restore_policy {
                    0 => "fallback",
                    1 => "exact",
                    2 => "anchor",
                    _ => "n/a",
                }
            );
        }
        4 => {
            let names = [
                "ok",
                "io",
                "zip",
                "epub",
                "encrypted",
                "truncated",
                "unsupported",
                "memory",
                "parse",
                "missing",
                "stale",
            ];
            let _ = write!(
                b,
                "Reader {} / restore {}",
                names
                    .get(usize::from(e.reader_status))
                    .copied()
                    .unwrap_or("unavailable"),
                match e.restore_failure {
                    0 => "none",
                    2 => "invalid",
                    _ => "unavailable",
                }
            );
        }
        5 if e.book != 0 => {
            let _ = write!(b, "Expected {}:{}", e.spine, e.character);
        }
        5 => {
            let _ = write!(
                b,
                "Expected: {}",
                if e.record == 0 {
                    "no book target"
                } else {
                    "untrusted"
                }
            );
        }
        6 => {
            let _ = write!(
                b,
                "Saved {} / tried {}",
                orientation(e.saved_orientation),
                orientation(e.tried_orientation)
            );
        }
        7 if e.reader_open => {
            let _ = write!(b, "Actual {}:{} / open", e.actual_spine, e.actual_character);
        }
        7 => {
            let _ = b.write_str("Actual unavailable / open no");
        }
        8 if e.first_screen != UNAVAILABLE => {
            let _ = write!(
                b,
                "First paint {} / gen {}",
                screen(e.first_screen),
                e.first_generation
            );
        }
        8 => {
            let _ = b.write_str("First paint: unavailable");
        }
        9 if p.source != InputSource::Unavailable => {
            let _ = write!(
                b,
                "First input B{} {} @{}ms",
                p.button,
                action(p.action),
                p.milliseconds
            );
        }
        9 => {
            let _ = b.write_str("First input: unavailable");
        }
        10 if p.source != InputSource::Unavailable => {
            let _ = write!(b, "Screens {} -> {}", screen(p.before), screen(p.after));
        }
        10 => {
            let _ = b.write_str("Input screens: unavailable");
        }
        11 if h.source != InputSource::Unavailable => {
            let _ = write!(
                b,
                "To Home {} B{} {}",
                if h.source == InputSource::Physical {
                    "physical"
                } else {
                    "USB"
                },
                h.button,
                action(h.action)
            );
        }
        11 => {
            let _ = b.write_str("Reading to Home: unavailable");
        }
        12 if h.source != InputSource::Unavailable => {
            let _ = write!(
                b,
                "At {} ms / reader open {}",
                h.milliseconds,
                word(h.reader_open)
            );
        }
        12 => {
            let _ = b.write_str("Home event: unavailable");
        }
        _ => {}
    }
}
