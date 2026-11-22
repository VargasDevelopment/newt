use core::cell::Cell;
use newt_core::app::{Action, Screen};
use newt_device_ui::format_wake_evidence_line as format_line;
use newt_wake_evidence::*;
struct Reader {
    open: bool,
    calls: [Cell<u32>; 5],
}
impl Reader {
    fn new(open: bool) -> Self {
        Self {
            open,
            calls: core::array::from_fn(|_| Cell::new(0)),
        }
    }
    fn touch(&self, i: usize) {
        self.calls[i].set(self.calls[i].get() + 1);
    }
    fn counts(&self) -> [u32; 5] {
        core::array::from_fn(|i| self.calls[i].get())
    }
}
impl ReaderObservation for Reader {
    fn status(&self) -> u8 {
        self.touch(0);
        10
    }
    fn restore_failure(&self) -> u8 {
        self.touch(1);
        2
    }
    fn orientation(&self) -> u8 {
        self.touch(2);
        1
    }
    fn opened(&self) -> bool {
        self.touch(3);
        self.open
    }
    fn page(&self) -> (u32, u32) {
        self.touch(4);
        assert!(self.open);
        (u32::MAX, 42)
    }
}
#[test]
fn unavailable_and_non_pending_do_not_observe() {
    for available in [false, true] {
        for a in [
            Attempt::NotRequested,
            Attempt::Pending,
            Attempt::LargestBlockGuard,
            Attempt::FreeHeapGuard,
            Attempt::OpenFailed,
            Attempt::Succeeded,
            Attempt::SourceUnavailable,
        ] {
            if available && a == Attempt::Pending {
                continue;
            }
            let r = Reader::new(true);
            let mut e = Evidence {
                available,
                attempt: a,
                ..Evidence::default()
            };
            e.initial_reader_attempt(Attempt::Succeeded, &r, 2);
            assert_eq!(r.counts(), [0; 5]);
            assert_eq!(e.attempt as u8, a as u8);
            assert_eq!(e.restore_policy, 255);
        }
    }
}
#[test]
fn page_only_when_open_and_attempt_freezes() {
    for open in [false, true] {
        let r = Reader::new(open);
        let mut e = Evidence {
            available: true,
            attempt: Attempt::Pending,
            ..Evidence::default()
        };
        e.initial_reader_attempt(Attempt::Succeeded, &r, 2);
        e.initial_reader_attempt(Attempt::OpenFailed, &r, 0);
        assert_eq!(r.counts(), [1, 1, 1, 1, u32::from(open)]);
        assert_eq!(e.attempt as u8, 5);
        assert_eq!(e.restore_policy, 2);
        assert_eq!(
            (e.reader_status, e.restore_failure, e.tried_orientation),
            (10, 2, 1)
        );
        assert_eq!(e.reader_open, open);
        assert_eq!(
            (e.actual_spine, e.actual_character),
            if open { (u32::MAX, 42) } else { (0, 0) }
        );
    }
}
#[test]
fn nonzero_paint_and_independent_input_receipts_freeze() {
    let mut e = Evidence::default();
    e.completed_paint(Screen::Home, 7);
    e.consumed_input(
        InputSource::Physical,
        1,
        Action::Back,
        1,
        Screen::EpubReading,
        Screen::Home,
        true,
    );
    assert_eq!(e.first_screen, 255);
    assert_eq!(e.first_physical.source as u8, 0);
    e.available = true;
    e.completed_paint(Screen::Home, 0);
    assert_eq!(e.first_screen, 255);
    e.completed_paint(Screen::EpubReading, u32::MAX);
    e.completed_paint(Screen::Home, 1);
    assert_eq!((e.first_screen, e.first_generation), (4, u32::MAX));
    e.consumed_input(
        InputSource::Unavailable,
        1,
        Action::Back,
        1,
        Screen::EpubReading,
        Screen::Home,
        true,
    );
    assert_eq!(e.first_reading_home.source as u8, 0);
    e.consumed_input(
        InputSource::Usb,
        2,
        Action::Back,
        u32::MAX,
        Screen::EpubReading,
        Screen::Home,
        true,
    );
    assert_eq!(e.first_physical.source as u8, 0);
    e.consumed_input(
        InputSource::Physical,
        3,
        Action::Next,
        0,
        Screen::Home,
        Screen::Device,
        false,
    );
    e.consumed_input(
        InputSource::Physical,
        9,
        Action::Back,
        1,
        Screen::EpubReading,
        Screen::Home,
        false,
    );
    assert_eq!(
        (
            e.first_physical.milliseconds,
            e.first_physical.button,
            e.first_physical.action,
            e.first_physical.before,
            e.first_physical.after,
            e.first_physical.reader_open
        ),
        (0, 3, 1, 0, 2, false)
    );
    assert_eq!(
        (
            e.first_reading_home.source as u8,
            e.first_reading_home.milliseconds,
            e.first_reading_home.button,
            e.first_reading_home.reader_open
        ),
        (2, u32::MAX, 2, true)
    );
}
#[test]
fn bounded_literals_preserve_every_tail_byte() {
    let e = Evidence::default();
    let literals = [
        "Reset 0 / wake 0 / eligible no",
        "Record ? / rejected ? / save ?",
        "untrusted -> unavailable / restored no",
        "Open not requested / n/a",
        "Reader unavailable / restore unavailable",
        "Expected: untrusted",
        "Saved unavailable / tried unavailable",
        "Actual unavailable / open no",
        "First paint: unavailable",
        "First input: unavailable",
        "Input screens: unavailable",
        "Reading to Home: unavailable",
        "Home event: unavailable",
        "",
    ];
    for (index, text) in literals.iter().enumerate() {
        let line = if index == 13 { u32::MAX } else { index as u32 };
        for cap in [0, 1, 2, 7, 16, 64, 96] {
            let mut out = [0xa5; 104];
            format_line(&e, line, &mut out[..cap]);
            let mut want = [0xa5; 104];
            if cap > 0 {
                let n = text.len().min(cap - 1);
                want[..n].copy_from_slice(&text.as_bytes()[..n]);
                want[n] = 0;
            }
            assert_eq!(out, want, "line {line}, cap {cap}");
        }
    }
}
#[test]
fn canonical_app_discriminants() {
    assert_eq!(
        [
            Screen::Home as u8,
            Screen::Reading as u8,
            Screen::Device as u8,
            Screen::Sleeping as u8,
            Screen::EpubReading as u8,
            Screen::TinyApp as u8,
            Screen::Library as u8
        ],
        [0, 1, 2, 3, 4, 5, 6]
    );
    assert_eq!(
        [
            Action::Previous as u8,
            Action::Next as u8,
            Action::Activate as u8,
            Action::Back as u8,
            Action::Sleep as u8,
            Action::Read as u8
        ],
        [0, 1, 2, 3, 4, 5]
    );
}
