// SPDX-License-Identifier: MIT
//! Home's clock source and its three durable preferences (time zone,
//! clock format and Sleep screen).
//!
//! Firmware Home borrows one six-byte HH:MM snapshot. A full Home composition
//! refreshes it once; focus and status passes keep the time already shown. The
//! published zone, format and Sleep screen choices change only through the
//! startup restore or a verified commit. The SD records are the only authority
//! for every reset class; there is no RTC copy, so a boot without the card
//! shows the product defaults, as a cold boot does.
use crate::{DeviceInfo, SleepScreenChoices, SystemBar};
use core::ffi::CStr;
use newt_core::clock_format::ClockFormat;
use newt_core::ds3231::valid_clock_utc;
use newt_core::durable::{
    Backend, ClockFormatCodec, Codec, Commit, SleepScreenCodec, TimeZoneCodec, commit_publish,
    restore,
};
use newt_core::time_zone::{ClockTime, TimeZone, local_clock_time};

/// Why the clock is read; DEV telemetry names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockReason {
    Boot,
    Home,
    System,
}

impl ClockReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Home => "home",
            Self::System => "system",
        }
    }
}

/// One bounded clock read into HH:MM's five visible bytes, local to `zone`.
/// `C` is the caller's bus context, borrowed for this call only; `format` is
/// the published format, for DEV telemetry. The source owns the terminator.
pub type ClockReader<C> = fn(&mut C, ClockReason, TimeZone, ClockFormat, &mut [u8; 5]);

/// What one Home paint shows: this source's snapshot, as `begin_paint` returns
/// it, or other caller text (an unrelated string or none). Naming the snapshot
/// avoids borrowing the source while a paint refreshes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HomeClockText<'a> {
    Snapshot,
    Other(Option<&'a CStr>),
}

const READ_FOR_PAINT: u8 = 1;
const TWELVE_HOUR: u8 = 2;
// The Sleep screen record's bits (progress 1, week 2, habit 4), shifted.
const SLEEP_SHIFT: u32 = 2;
const SLEEP_BITS: u8 = 7 << SLEEP_SHIFT;

/// The published Home clock. The latch, format and Sleep screen choices share
/// one byte and the zone another, so the reader and snapshot are the only
/// other state: 12 bytes on RV32, as the original.
pub struct HomeClockSource<C> {
    reader: Option<ClockReader<C>>,
    text: [u8; 6],
    flags: u8,
    zone: TimeZone,
}

const _: () = assert!(
    size_of::<HomeClockSource<()>>() == if size_of::<usize>() == 4 { 12 } else { 16 },
    "the zone, format and Sleep screen use padding, not RAM"
);

impl<C> HomeClockSource<C> {
    pub const fn new() -> Self {
        Self {
            reader: None,
            text: *b"--:--\0",
            flags: 0,
            zone: TimeZone::Utc,
        }
    }

    pub const fn zone(&self) -> TimeZone {
        self.zone
    }

    pub const fn format(&self) -> ClockFormat {
        if self.flags & TWELVE_HOUR != 0 {
            ClockFormat::TwelveHour
        } else {
            ClockFormat::TwentyFourHour
        }
    }

    pub const fn sleep_screen(&self) -> SleepScreenChoices {
        let bits = self.flags >> SLEEP_SHIFT;
        SleepScreenChoices {
            progress: bits & 1 != 0,
            week: bits & 2 != 0,
            habit: bits & 4 != 0,
        }
    }

    /// The HH:MM snapshot in the published zone, or --:-- until a valid read.
    pub fn text(&self) -> &CStr {
        // Readers never reach byte 5, so the terminator is always present.
        CStr::from_bytes_until_nul(&self.text).unwrap_or(c"--:--")
    }

    /// Startup with separate store/context borrows: publish the durable zone, format and Sleep screen
    /// choices, then read once for boot, so the boot time and every later Home
    /// paint are local and in the chosen format. None disables every read.
    /// `store` and `ctx` are separate borrows for this call only.
    pub fn begin_home_clock<B: Backend>(
        &mut self,
        store: &mut B,
        ctx: &mut C,
        reader: Option<ClockReader<C>>,
    ) {
        self.restore_preferences(store);
        self.begin_reader(ctx, reader);
    }

    /// Restore the three durable choices in the pinned order. The firmware
    /// can end every store loan before borrowing the Board for its Boot read.
    pub fn restore_preferences<B: Backend>(&mut self, store: &mut B) {
        self.restore_time_zone(store);
        self.restore_clock_format(store);
        self.restore_sleep_screen(store);
    }

    /// Finish startup after restore_preferences; performs exactly one Boot read.
    #[inline(never)]
    pub fn begin_reader(&mut self, ctx: &mut C, reader: Option<ClockReader<C>>) {
        self.reader = reader;
        if let Some(read) = reader {
            self.read_snapshot(ctx, read, ClockReason::Boot);
        }
    }

    /// Start one Home paint; its full composition may read once.
    pub fn begin_paint(&mut self) -> HomeClockText<'static> {
        self.flags &= !READ_FOR_PAINT;
        HomeClockText::Snapshot
    }

    /// One full Home composition. Only this source's snapshot is refreshed,
    /// once per begin_paint. The latch is set before the read, so a rejected
    /// overlay's fallback cannot read again. Focus and status passes never call
    /// this.
    #[inline(never)]
    pub fn paint(&mut self, ctx: &mut C, shown: HomeClockText<'_>) {
        if !matches!(shown, HomeClockText::Snapshot) || self.flags & READ_FOR_PAINT != 0 {
            return;
        }
        let Some(read) = self.reader else {
            return;
        };
        self.flags |= READ_FOR_PAINT;
        self.read_snapshot(ctx, read, ClockReason::Home);
    }

    /// The text a paint draws once its paint call returned.
    pub fn shown<'a>(&'a self, clock: HomeClockText<'a>) -> Option<&'a CStr> {
        match clock {
            HomeClockText::Snapshot => Some(self.text()),
            HomeClockText::Other(text) => text,
        }
    }

    /// The bar for one SystemPage paint: one uncached read in the published
    /// zone that leaves Home's latch and snapshot alone; --:-- before the
    /// source starts. `radio_on` is the Wi-Fi flash fence for this paint, as
    /// the NVS guard owner reports it (flash_fenced).
    pub fn read_system_bar(&self, ctx: &mut C, info: &DeviceInfo<'_>, radio_on: bool) -> SystemBar {
        let mut bar = SystemBar::new(
            None,
            self.format(),
            info.battery_known,
            info.battery_percent,
        );
        bar.radio_on = radio_on;
        if let (Some(read), Some(digits)) = (self.reader, bar.clock.first_chunk_mut::<5>()) {
            read(ctx, ClockReason::System, self.zone, self.format(), digits);
        }
        bar
    }

    /// Lend this source to another bus context for one scope, as the mounted
    /// reader scope borrows the Board that `C` owns. The published choices,
    /// snapshot and paint latch go in and come back, so nothing is restored
    /// or read here. `reader` serves the lent source only if this one has a
    /// reader. Published choices and the paint latch return on unwind too.
    pub fn with_context<D, R>(
        &mut self,
        reader: ClockReader<D>,
        scope: impl FnOnce(&mut HomeClockSource<D>) -> R,
    ) -> R {
        let lent = HomeClockSource {
            reader: self.reader.map(|_| reader),
            text: self.text,
            flags: self.flags,
            zone: self.zone,
        };
        struct Context<'a, C, D> {
            original: &'a mut HomeClockSource<C>,
            lent: HomeClockSource<D>,
        }
        impl<C, D> Drop for Context<'_, C, D> {
            fn drop(&mut self) {
                // A verified commit cannot be rolled back by a caught panic.
                let HomeClockSource {
                    reader: _,
                    text,
                    flags,
                    zone,
                } = &self.lent;
                self.original.text = *text;
                self.original.flags = *flags;
                self.original.zone = *zone;
            }
        }
        let mut loan = Context {
            original: self,
            lent,
        };
        scope(&mut loan.lent)
    }

    /// Missing, damaged or unreadable storage shows UTC. Nothing here writes or
    /// repairs the record.
    #[inline(never)]
    fn restore_time_zone<B: Backend>(&mut self, store: &mut B) {
        let id = restore::<B, TimeZoneCodec, 20>(store, TimeZone::Utc as u32);
        self.zone = TimeZone::from_id(id).unwrap_or(TimeZone::Utc);
    }

    /// Missing, damaged or unreadable storage shows 24-hour, unrepaired.
    #[inline(never)]
    fn restore_clock_format<B: Backend>(&mut self, store: &mut B) {
        let id = restore::<B, ClockFormatCodec, 20>(store, ClockFormat::TwentyFourHour as u32);
        self.publish_format(ClockFormat::from_id(id).unwrap_or(ClockFormat::TwentyFourHour));
    }

    /// Missing, damaged or unreadable storage shows every choice Off, unrepaired.
    #[inline(never)]
    fn restore_sleep_screen<B: Backend>(&mut self, store: &mut B) {
        self.publish_sleep(restore::<B, SleepScreenCodec, 20>(store, 0));
    }

    /// Publish only a verified commit; until then the previous zone stays live.
    /// Unconfirmed is never "unsaved": a reboot may restore the old or new value.
    /// An explicit UTC is written, so it supersedes an older durable zone.
    #[inline(never)]
    pub fn commit_time_zone<B: Backend>(&mut self, store: &mut B, zone: TimeZone) -> Commit {
        let committed = commit::<B, TimeZoneCodec>(store, zone as u32, self.zone as u32);
        if committed == Commit::Saved {
            self.zone = zone;
        }
        committed
    }

    /// As commit_time_zone; an explicit 24-hour supersedes an older 12-hour.
    #[inline(never)]
    pub fn commit_clock_format<B: Backend>(
        &mut self,
        store: &mut B,
        format: ClockFormat,
    ) -> Commit {
        let committed = commit::<B, ClockFormatCodec>(store, format as u32, self.format() as u32);
        if committed == Commit::Saved {
            self.publish_format(format);
        }
        committed
    }

    /// One record holds all three choices; an explicit all-Off supersedes an
    /// older opt-in.
    #[inline(never)]
    pub fn commit_sleep_screen<B: Backend>(
        &mut self,
        store: &mut B,
        choices: SleepScreenChoices,
    ) -> Commit {
        let intended = sleep_record(choices);
        let committed = commit::<B, SleepScreenCodec>(store, intended, self.sleep_record());
        if committed == Commit::Saved {
            self.publish_sleep(intended);
        }
        committed
    }

    fn read_snapshot(&mut self, ctx: &mut C, read: ClockReader<C>, reason: ClockReason) {
        let (zone, format) = (self.zone, self.format());
        if let Some(digits) = self.text.first_chunk_mut::<5>() {
            read(ctx, reason, zone, format, digits);
        }
    }

    fn publish_format(&mut self, format: ClockFormat) {
        self.flags &= !TWELVE_HOUR;
        if format == ClockFormat::TwelveHour {
            self.flags |= TWELVE_HOUR;
        }
    }

    // `record` is a decoded or encoded Sleep screen value, so at most 7.
    fn publish_sleep(&mut self, record: u32) {
        self.flags = (self.flags & !SLEEP_BITS) | (((record as u8) & 7) << SLEEP_SHIFT);
    }

    fn sleep_record(&self) -> u32 {
        u32::from((self.flags >> SLEEP_SHIFT) & 7)
    }
}

fn sleep_record(choices: SleepScreenChoices) -> u32 {
    u32::from(choices.progress) | (u32::from(choices.week) << 1) | (u32::from(choices.habit) << 2)
}

// The existing verified publication over one scalar record. The caller keeps
// its typed runtime value and publishes it only for Saved.
fn commit<B: Backend, R: Codec<20, Value = u32>>(
    store: &mut B,
    intended: u32,
    shown: u32,
) -> Commit {
    let mut shown = shown;
    commit_publish::<B, R, 20>(store, intended, &mut shown)
}

/// The pinned Home read over one bridge result; `raw` is None when the bounded
/// transaction failed. Writes HH:MM in `zone`, else --:-- for a failed,
/// implausible or oscillator-stopped snapshot. True when the time is valid.
#[inline(never)]
pub fn read_home_clock(raw: Option<&[u8; 16]>, zone: TimeZone, text: &mut [u8; 5]) -> bool {
    *text = *b"--:--";
    let Some(utc) = raw.and_then(valid_clock_utc) else {
        return false;
    };
    local_clock_text(utc, zone, text);
    true
}

// Converting outside read_home_clock keeps its frame small.
#[inline(never)]
fn local_clock_text(utc: ClockTime, zone: TimeZone, text: &mut [u8; 5]) {
    let local = local_clock_time(utc, zone);
    *text = [
        b'0' + local.hour / 10,
        b'0' + local.hour % 10,
        b':',
        b'0' + local.minute / 10,
        b'0' + local.minute % 10,
    ];
}

/// The longest bytes of each pinned `CLOCK` printf call: its format's fixed
/// bytes plus each field's longest value. `[84, 47]`: the prefix through
/// `status=` with reason `system`, and the suffix from ` zone=` with
/// `australia-eastern` and `12h`; `osf=-1` and `local=--:--` are already the
/// longest of theirs. The SDK formats any printf longer than 63 bytes, so every
/// pinned prefix, into a heap buffer; Rust stages both halves on the stack.
#[cfg(feature = "dev-usb")]
pub const CLOCK_HALF_BOUNDS: [usize; 2] = clock_half_bounds();

#[cfg(feature = "dev-usb")]
const fn clock_half_bounds() -> [usize; 2] {
    use newt_core::clock_format::clock_format_id;
    const fn longest(values: &[&str]) -> usize {
        let (mut index, mut most) = (0, 0);
        while index < values.len() {
            if values[index].len() > most {
                most = values[index].len();
            }
            index += 1;
        }
        most
    }
    let reasons = [
        ClockReason::Boot.as_str(),
        ClockReason::Home.as_str(),
        ClockReason::System.as_str(),
    ];
    let formats = [
        clock_format_id(ClockFormat::TwentyFourHour),
        clock_format_id(ClockFormat::TwelveHour),
    ];
    // The longest time_zone_id, which is not const; the policy tests check
    // every zone against this bound.
    let zone = "australia-eastern".len();
    [
        "CLOCK reason= read=0 osf=-1 valid=0 raw=00-00-00T00:00:00 weekday=00 status=00".len()
            + longest(&reasons),
        " zone= format= local=--:--\n".len() + zone + longest(&formats),
    ]
}

/// write_clock_log's one stack buffer, which both halves reuse.
#[cfg(feature = "dev-usb")]
const CLOCK_HALF_BYTES: usize = {
    let [prefix, suffix] = CLOCK_HALF_BOUNDS;
    if prefix > suffix { prefix } else { suffix }
};

/// One `CLOCK` half staged on write_clock_log's stack.
#[cfg(feature = "dev-usb")]
struct ClockHalf {
    bytes: [u8; CLOCK_HALF_BYTES],
    len: usize,
}

#[cfg(feature = "dev-usb")]
impl ClockHalf {
    /// Stage one half from empty, then pass it to `out` in one write_str, as
    /// each pinned printf makes one SDK write. A half that does not fit is not
    /// written; no input reaches that (policy tests).
    fn send(
        &mut self,
        out: &mut impl core::fmt::Write,
        stage: impl FnOnce(&mut Self) -> core::fmt::Result,
    ) -> core::fmt::Result {
        self.len = 0;
        stage(self)?;
        // Every staged byte came from a whole &str, so the half is UTF-8.
        let staged = core::str::from_utf8(&self.bytes[..self.len]).map_err(|_| core::fmt::Error)?;
        out.write_str(staged)
    }
}

#[cfg(feature = "dev-usb")]
impl core::fmt::Write for ClockHalf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len + s.len();
        let slot = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        slot.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// DEV telemetry for one read, in the pinned two printf calls' bytes and
/// split: the prefix through `status=`, then the suffix from ` zone=`, each
/// passed to `out` in one `write_str`. The pinned code ignores both printf
/// results, so the suffix is attempted even when the prefix failed; Err if
/// either failed, never retried. A failed read preserves initialized partial
/// bytes, as the pinned reader does. Not inlined, so the staging buffer stays
/// out of the read's frame.
#[cfg(feature = "dev-usb")]
#[inline(never)]
pub fn write_clock_log(
    out: &mut impl core::fmt::Write,
    reason: ClockReason,
    raw: &[u8; 16],
    complete: bool,
    valid: bool,
    zone: TimeZone,
    format: ClockFormat,
    text: &[u8; 5],
) -> core::fmt::Result {
    use core::fmt::Write as _;
    use newt_core::clock_format::clock_format_id;
    use newt_core::ds3231::OSCILLATOR_STOPPED;
    use newt_core::time_zone::time_zone_id;
    let r = raw;
    let osf = if complete {
        i8::from(raw[15] & OSCILLATOR_STOPPED != 0)
    } else {
        -1
    };
    let half = &mut ClockHalf {
        bytes: [0; CLOCK_HALF_BYTES],
        len: 0,
    };
    let prefix = half.send(out, |half| {
        write!(
            half,
            "CLOCK reason={} read={} osf={} valid={} raw={:02x}-{:02x}-{:02x}T{:02x}:{:02x}:{:02x} weekday={:02x} status={:02x}",
            reason.as_str(),
            u8::from(complete),
            osf,
            u8::from(valid),
            r[6],
            r[5],
            r[4],
            r[2],
            r[1],
            r[0],
            r[3],
            r[15]
        )
    });
    let suffix = half.send(out, |half| {
        write!(
            half,
            " zone={} format={} local=",
            time_zone_id(zone),
            clock_format_id(format)
        )?;
        for &byte in text.iter().take_while(|&&byte| byte != 0) {
            half.write_char(char::from(byte))?;
        }
        half.write_char('\n')
    });
    prefix.and(suffix)
}
