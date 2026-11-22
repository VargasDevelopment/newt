// SPDX-License-Identifier: MIT

//! Home's durable choices over the actual Board. The existing store's mount
//! gate runs at every namespace begin, including readonly reconciliation.
//! The Board loan encloses every descriptor's full policy lifetime; individual
//! synchronous native calls use short registered I/O views. No filesystem port
//! is minted before mounting succeeds. A mounted reader scope borrows the
//! same boot-wide owners. Not firmware main or power acceptance.
#[path = "reader_preferences.rs"]
pub mod preferences;
#[path = "home_startup/boot_resume.rs"]
pub mod boot_resume;
#[path = "home_startup/statistics.rs"]
mod statistics;
#[path = "home_startup/state.rs"]
mod state;
pub(crate) use state::with_state_parts;
use crate::board_clock::{BoardBus, PanelBus, read_board_clock, read_panel_clock};
use crate::nvs::{self, NvsBlobBackend};
use core::fmt::Write;
use newt_c3_board::Board;
use newt_core::clock_format::ClockFormat;
use newt_core::durable::{Backend, Commit};
use newt_core::time_zone::TimeZone;
use newt_core::update_online::TestBuilds;
use newt_device_ui::SleepScreenChoices;
use newt_device_ui::home_clock::{ClockReader, HomeClockSource};
use newt_native_io::{OsNative, UiIo};
use newt_native_storage::reading_state::{
    FIRMWARE_STATE, FIRMWARE_STATS, ReadingStateRouter, StateFacts, StateMedium,
};
use newt_native_storage::{Error, Native, Open, Result, Stat};

/// A port used synchronously within the same exclusive Board loan. Firmware
/// uses UiIo::port; host fixtures relocate the card without changing policy.
/// Every view from one card must share its descriptor/nonce domain; the
/// enclosing HomePort retains descriptor ownership until the router ends.
pub trait CardPort {
    type Port<'ui>: Native;
    fn port<'ui>(&mut self, io: &'ui UiIo) -> Self::Port<'ui>;
}

pub struct FirmwareCard;
impl CardPort for FirmwareCard {
    type Port<'ui> = OsNative<'ui>;
    fn port<'ui>(&mut self, io: &'ui UiIo) -> OsNative<'ui> {
        io.port()
    }
}

/// Kept until the record/router has closed all descriptors. No listing is
/// supported by this private transport; only the router's namespace-listing
/// implementation requires Directory.
pub(crate) struct HomePort<'op, 'sdk, P> {
    board: &'op mut Board<'sdk>,
    card: &'op mut P,
}
impl<P: CardPort> HomePort<'_, '_, P> {
    fn access<R>(&mut self, operation: impl for<'ui> FnOnce(&mut P::Port<'ui>) -> R) -> Result<R> {
        let Self { board, card } = self;
        board
            .with_reader_io(|_, io| operation(&mut card.port(io)))
            .map_err(|_| Error::Unavailable)
    }
}
impl<P: CardPort> Native for HomePort<'_, '_, P> {
    fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
        self.access(|port| port.open(path, mode))?
    }
    fn close(&mut self, fd: i32) -> Result<()> {
        self.access(|port| port.close(fd))?
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> {
        self.access(|port| port.read(fd, out))?
    }
    fn write(&mut self, fd: i32, input: &[u8]) -> Result<usize> {
        self.access(|port| port.write(fd, input))?
    }
    fn seek(&mut self, fd: i32, offset: u32) -> Result<u64> {
        self.access(|port| port.seek(fd, offset))?
    }
    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()> {
        self.access(|port| port.truncate(fd, bytes))?
    }
    fn sync(&mut self, fd: i32) -> Result<()> {
        self.access(|port| port.sync(fd))?
    }
    fn fstat(&mut self, fd: i32) -> Result<Stat> {
        self.access(|port| port.fstat(fd))?
    }
    fn stat(&mut self, path: &str, no_follow: bool) -> Result<Stat> {
        self.access(|port| port.stat(path, no_follow))?
    }
    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.access(|port| port.mkdir(path))?
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        self.access(|port| port.unlink(path))?
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.access(|port| port.rename(from, to))?
    }
    fn next_nonce(&mut self) -> u32 {
        // State records use their boot-lived StateFacts nonce. This remains a
        // real native delegation when called inside the prepared transaction.
        self.access(|port| port.next_nonce()).unwrap_or(0)
    }
}

pub(crate) struct HomeMedium;
impl<'op, 'sdk, P: CardPort> StateMedium<HomePort<'op, 'sdk, P>> for HomeMedium {
    const STATE: &'static str = FIRMWARE_STATE;
    const STATS: &'static str = FIRMWARE_STATS;
    fn release_pages_for_state(&mut self) {
        // Home owns the Board here; no reader scope or Pages loan can coexist.
    }
    fn mount(&mut self, native: Option<&mut HomePort<'op, 'sdk, P>>) -> bool {
        native.is_some_and(|port| port.board.ensure_sd_mounted().is_ok())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HomeChoice {
    Zone(TimeZone),
    Format(ClockFormat),
    SleepScreen(SleepScreenChoices),
    TestBuilds(TestBuilds),
}
impl HomeChoice {
    fn commit<B: Backend, C>(self, clock: &mut HomeClockSource<C>, store: &mut B) -> Commit {
        match self {
            Self::Zone(zone) => clock.commit_time_zone(store, zone),
            Self::Format(format) => clock.commit_clock_format(store, format),
            Self::SleepScreen(choices) => clock.commit_sleep_screen(store, choices),
            Self::TestBuilds(choice) => clock.commit_test_builds(store, choice),
        }
    }
}

/// One mounted reader scope's view of the boot-wide owners: the sole panel as
/// the lent Home clock's context, the registered UiIo, and this boot's
/// StateFacts, NVS and card. Every borrow ends with the scope, so neither the
/// panel, a native port nor the lent clock can leave it; the clock's choices,
/// snapshot and latch return to Home's source when it does.
pub struct ReaderScope<'c, 'p, 'scope, 'sdk, L, N, P> {
    pub clock: &'c mut HomeClockSource<PanelBus<'p, 'scope, 'sdk, &'p mut L>>,
    pub bus: PanelBus<'p, 'scope, 'sdk, &'p mut L>,
    pub io: &'c UiIo,
    pub facts: &'c mut StateFacts,
    pub preferences: &'c mut preferences::ReaderPreferences,
    pub nvs: &'c mut N,
    pub card: &'c mut P,
}

/// Facts and the clock persist for the whole boot; record transactions borrow
/// these owners. No return to Home restores preferences a second time.
pub struct HomeRuntime<'sdk, L, N, P> {
    pub bus: BoardBus<'sdk, L>,
    pub clock: HomeClockSource<BoardBus<'sdk, L>>,
    pub facts: StateFacts,
    pub preferences: preferences::ReaderPreferences,
    pub nvs: N,
    pub card: P,
}
impl<'sdk, L: Write, N: nvs::Native, P: CardPort> HomeRuntime<'sdk, L, N, P> {
    pub fn start(bus: BoardBus<'sdk, L>, nvs: N, card: P) -> Self where N: nvs::Inventory {
        let mut home = Self::restore_durable(bus, nvs, card);
        home.begin_clock();
        home
    }

    /// Restore durable choices before input/task startup. The app begins the
    /// clock after the frozen battery/Wire initialization point.
    pub fn restore_durable(bus: BoardBus<'sdk, L>, nvs: N, card: P) -> Self where N: nvs::Inventory {
        let mut home = Self {
            bus,
            clock: HomeClockSource::new(),
            facts: StateFacts::new(),
            preferences: preferences::ReaderPreferences::default(),
            nvs,
            card,
        };
        home.preserve_legacy();
        home.begin();
        home
    }

    #[inline(never)]
    fn preserve_legacy(&mut self) where N: nvs::Inventory {
        // Board acquisition already tried mounting; do not retry per record.
        if !self.bus.board.sd_mounted() { return; }
        let mut native = HomePort { board: &mut self.bus.board, card: &mut self.card };
        let report = nvs::reclaim::run(&mut self.nvs, &mut self.facts, &mut HomeMedium, &mut native);
        if report.kept != 0 {
            let _ = writeln!(self.bus.log, "Legacy NVS kept: {}", report.kept);
        }
    }

    fn begin(&mut self) {
        self.restore_reader_preferences();
        {
            let mut native = HomePort {
                board: &mut self.bus.board,
                card: &mut self.card,
            };
            let mut medium = HomeMedium;
            let mut store = ReadingStateRouter::new(
                NvsBlobBackend::new(&mut self.nvs),
                &mut self.facts,
                &mut medium,
                &mut native,
            );
            self.clock.restore_preferences(&mut store);
        }
    }

    pub fn begin_clock(&mut self) {
        self.clock
            .begin_reader(&mut self.bus, Some(read_board_clock));
    }

    /// Frozen setup reads NWBR once, before selected-peer and page arbitration.
    /// The same setup-owned snapshot feeds both decisions; warm boots do no I/O.
    pub fn load_boot_resume(
        &mut self,
        power_on: bool,
        notice: &mut newt_core::save_notice::ReadingSaveNotice,
    ) -> boot_resume::Target {
        let settings = self.preferences.settings();
        self.with_state_store(|_, store| boot_resume::Target::load(store, power_on, settings, notice))
    }

    /// The same startup load is used after component state is discarded or
    /// stale. Durable NVS, never the previous runtime object, is authority.
    pub fn restore_reader_preferences(&mut self) {
        let loaded = self.preferences.restore(&mut self.nvs);
        if loaded.is_some_and(|error| error != newt_core::preferences_store::StoreError::Empty) {
            let _ = writeln!(
                self.bus.log,
                "Reader preferences unavailable; session defaults only, saves blocked"
            );
        }
        self.choose_reader_orientation();
    }

    /// Frozen chooseReaderOrientation: retry before the first history write
    /// when the startup card history was unknown. No history is written here.
    pub fn choose_reader_orientation(&mut self) {
        if !self.preferences.orientation_pending() {
            return;
        }
        let history = {
            let mut native = HomePort {
                board: &mut self.bus.board,
                card: &mut self.card,
            };
            let mut medium = HomeMedium;
            let mut store = ReadingStateRouter::new(
                NvsBlobBackend::new(&mut self.nvs),
                &mut self.facts,
                &mut medium,
                &mut native,
            );
            let folder = store.state_folder();
            preferences::card_history(&mut store, folder)
        };
        if self.preferences.choose_orientation(&mut self.nvs, history) == Some(false) {
            let _ = writeln!(
                self.bus.log,
                "Reader orientation default unconfirmed; session keeps the defaults"
            );
        }
    }

    /// #272 main.cpp:554 / quick_start_runtime.h::beginQuickStart. Call after
    /// Home::start and the real reader reopen decision, before the first paint.
    /// Resume reads only; settle history before any NWQS write creates State.
    /// State/facts are caller loans, never added to Home or AppState storage.
    pub fn begin_quick_start(
        &mut self,
        app: &mut newt_core::app::AppState,
        shown: &mut newt_core::quick_start::State,
        facts: &mut newt_core::quick_start::Facts,
        reader_reopened: bool,
    ) -> bool {
        use newt_core::quick_start::{self as quick, CardHistory, State};
        if reader_reopened { return false; }
        let resume = self.with_state_store(|_, store| {
            quick::resume(store, |store| {
                let folder = store.state_folder();
                match preferences::card_history(store, folder) {
                    preferences::CardHistory::Unknown => CardHistory::Unknown,
                    preferences::CardHistory::New => CardHistory::New,
                    preferences::CardHistory::RanNewt => CardHistory::RanNewt,
                }
            })
        });
        if !resume.show { return false; }
        self.choose_reader_orientation();
        *shown = State { from_settings: resume.at.from_settings, ..State::default() };
        // Frozen begin clears the step to Welcome before quickStartFacts(),
        // so that call does not probe Wi-Fi even when the recorded step is Wifi.
        *facts = quick::Facts {
            orientation: self.preferences.settings().orientation,
            zone: self.clock.zone(),
            ..quick::Facts::default()
        };
        quick::enter_step(shown, facts, resume.at.step);
        app.screen = newt_core::app::Screen::QuickStart;
        true
    }

    pub fn commit(&mut self, choice: HomeChoice) -> Commit {
        self.with_state_store(|clock, store| choice.commit(clock, store))
    }

    /// NWQS uses the same boot-wide SD authority and bounded commit policy.
    /// The single #272 namespace extension lives with the existing router.
    pub fn commit_quick_start(&mut self, record: newt_core::quick_start::Record) -> Commit {
        self.with_state_store(|_, store| newt_core::quick_start::commit(store, record))
    }

    /// Sealed quick_start_store.h:100: inspect only the sealed card record,
    /// never the NVS key or plaintext network, including in the bench image.
    pub fn wifi_network_saved(&mut self) -> bool {
        use newt_core::{durable::{Load, RecordStore}, wifi_network_store::{WifiNetworkCodec, BYTES}};
        self.with_state_store(|_, store| {
            matches!(RecordStore::<_, WifiNetworkCodec, BYTES>::new(store).load(),
                Load::Loaded(sealed) if sealed.network)
        })
    }

    pub fn load_seen_build(&mut self) -> newt_core::durable::Load<newt_core::update_seen::NewtAppIdentity> {
        use newt_core::{durable::RecordStore, update_seen::{SeenBuildCodec, BYTES}};
        self.with_state_store(|_, store| RecordStore::<_, SeenBuildCodec, BYTES>::new(store).load())
    }

    /// A failed note is deliberately ignored by the caller: an old or new
    /// complete record may reappear next boot, as frozen finishFirstPaint.
    pub fn note_seen_build(&mut self, running: newt_core::update_seen::NewtAppIdentity) {
        use newt_core::{durable::RecordStore, update_seen::{SeenBuildCodec, BYTES}};
        self.with_state_store(|_, store| {
            let _ = RecordStore::<_, SeenBuildCodec, BYTES>::new(store).save(running);
        });
    }

    /// The operation ends before Home regains its owners; no backend getter
    /// or stored loan is exposed. Zone and NWQS share one router constructor.
    pub(crate) fn with_state_store<R>(
        &mut self,
        operation: impl FnOnce(
            &mut HomeClockSource<BoardBus<'sdk, L>>,
            &mut ReadingStateRouter<'_, NvsBlobBackend<'_, N>, HomePort<'_, 'sdk, P>, HomeMedium>,
        ) -> R,
    ) -> R {
        let Self { bus, nvs, card, facts, clock, .. } = self;
        with_state_parts(&mut bus.board, nvs, card, facts, |store| operation(clock, store))
    }

    /// The selected file and revision owners may survive returns to Home,
    /// as frozen LibraryReaderSelection does. Their native loans cannot leave
    /// this outer UI scope; the operation returns before rail/power shutdown.
    /// A late card is admitted once here without rebuilding Home's choices.
    pub fn with_storage_io<R>(
        &mut self,
        operation: impl for<'io> FnOnce(&mut Self, &'io UiIo) -> R,
    ) -> core::result::Result<R, newt_c3_board::Error> {
        let domain = self.bus.board.mounted_io()?;
        Ok(domain.with_io(|io| operation(self, io)))
    }

    /// One mounted reader scope over this boot's owners. A missing card is
    /// retried here, before any descriptor exists, and rejects the scope with
    /// every owner unchanged; a card that arrives late restores nothing.
    /// State work inside belongs on MountedStateMedium, which never remounts
    /// beneath the scope's descriptors.
    pub fn with_reader_io<R>(
        &mut self,
        operation: impl FnOnce(ReaderScope<'_, '_, '_, 'sdk, L, N, P>) -> R,
    ) -> core::result::Result<R, newt_c3_board::Error> {
        self.bus.board.ensure_sd_mounted()?;
        let Self {
            bus: BoardBus { board, log },
            clock,
            facts,
            preferences,
            nvs,
            card,
        } = self;
        board.with_reader_io(|panel, io| {
            let bus = PanelBus { panel, log };
            let reader: ClockReader<PanelBus<'_, '_, 'sdk, &mut L>> = read_panel_clock;
            clock.with_context(reader, |clock| {
                operation(ReaderScope {
                    clock,
                    bus,
                    io,
                    facts,
                    preferences,
                    nvs,
                    card,
                })
            })
        })
    }
}
