// SPDX-License-Identifier: MIT
//! The card app flow through the firmware's own owners
//! (app_runtime/installed_app.rs and card_app.rs): the boot restore, entering
//! Apps, the offer's choices and Install, over the fake NVS and fake card.
//! A restart drops every owner and restores over the durable bytes alone,
//! into storage that still holds a stale app, as RAM might.
use crate::card::Card;
use crate::card_app::{self, Prompt};
use crate::installed_app::InstalledApp;
use crate::{CORRUPT, Nvs, Put, bundled, corrupt, definition, record, titled};
use newt_core::app::Action;
use newt_core::app_store::{PACKAGE_BYTES, Refusal};
use newt_core::durable::StorageOpen;
use newt_tiny_app::{Definition, State, install::{Choice, Page, Problem}, pause::BUILTIN_BYTES};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// The system allocator, except that this thread can refuse one size.
struct Allocator;
thread_local! {
    static REFUSE: Cell<Option<usize>> = const { Cell::new(None) };
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if REFUSE.get() == Some(layout.size()) {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn refusing<T>(size: usize, work: impl FnOnce() -> T) -> T {
    REFUSE.set(Some(size));
    let result = work();
    REFUSE.set(None);
    result
}

/// One boot's owners: the installed app over boot-owned storage, the RAM-only
/// prompt and Tiny's selection.
struct Boot {
    installed: InstalledApp,
    prompt: Prompt,
    tiny: State,
}

impl Boot {
    /// Startup's restore, into storage still holding a stale app.
    fn new(nvs: &mut Nvs) -> Self {
        let storage = Box::leak(Box::new(definition(&titled(b"Stale"))));
        Self { installed: InstalledApp::restore(nvs, storage), prompt: Prompt::default(), tiny: State::default() }
    }

    fn running(&self) -> Option<Definition> { self.installed.running().copied() }

    /// Apps opened over `card`; whether a page is shown.
    fn enter(&mut self, card: &mut Card) -> bool {
        card_app::enter(&mut self.prompt, self.installed.running(), &mut card.clone(), card)
    }

    fn page(&self) -> Option<Page<'_>> { self.prompt.page() }

    fn press(&mut self, action: Action) -> Option<Choice> { self.prompt.choose(action) }

    fn install(&mut self, nvs: &mut Nvs, card: &mut Card) -> bool {
        assert_eq!(self.press(Action::Activate), Some(Choice::Install));
        let changed = card_app::install(&mut self.prompt, &mut self.installed, &mut self.tiny, nvs, card);
        assert!(nvs.session.is_none(), "the install closes its session");
        changed
    }
}

fn card(wire: &[u8; PACKAGE_BYTES]) -> Card { Card::new().file("app.newt", wire) }

/// A restart over the same durable bytes, with nothing else carried over.
fn restart(nvs: &Nvs) -> Option<Definition> {
    let mut cold = Nvs::with(nvs.slots[0].clone(), nvs.slots[1].clone());
    let running = Boot::new(&mut cold).running();
    assert!(cold.writes.is_empty() && cold.session.is_none());
    running
}

fn offered(boot: &Boot) -> Definition {
    match boot.page() {
        Some(Page::Offer(app)) => *app,
        other => panic!("an offer expected, got {other:?}"),
    }
}

#[test]
fn a_cold_boot_restores_the_installed_app_never_stale_ram() {
    let app = titled(b"Card");
    let mut nvs = Nvs::empty();
    let mut boot = Boot::new(&mut nvs);
    assert_eq!(boot.running(), Some(bundled()), "the record, not the stale storage");
    let mut files = card(&app);
    assert!(boot.enter(&mut files));
    assert_eq!(offered(&boot), definition(&app));
    assert!(boot.install(&mut nvs, &mut files));
    assert_eq!(boot.page(), Some(Page::Installed));
    assert_eq!(boot.running(), Some(definition(&app)));
    assert_eq!(restart(&nvs), Some(definition(&app)));
    // The next boot finds the same file: the running app is no offer.
    let mut next = Boot::new(&mut nvs);
    assert!(!next.enter(&mut files));
    assert_eq!(next.page(), None);
}

#[test]
fn missing_or_corrupt_storage_is_unavailable_until_an_install() {
    let app = titled(b"Card");
    let mut nvs = Nvs::with(Some(corrupt(1, b"Old")), None);
    let mut boot = Boot::new(&mut nvs);
    assert_eq!((boot.running(), boot.installed.unavailable()), (None, Some(CORRUPT.as_bytes())));
    let mut files = card(&app);
    assert!(boot.enter(&mut files), "with no running app, any valid app is offered");
    assert!(boot.install(&mut nvs, &mut files));
    assert_eq!((boot.page(), boot.running(), boot.installed.unavailable()), (Some(Page::Installed), Some(definition(&app)), None));
    assert_eq!(nvs.writes, [1], "the damaged slot is kept");
    assert_eq!(restart(&nvs), Some(definition(&app)));
    // Storage that cannot be opened: no write, and the app stays unavailable.
    let mut nvs = Nvs::empty();
    nvs.open = StorageOpen::Failed;
    let mut boot = Boot::new(&mut nvs);
    assert_eq!(boot.running(), None);
    assert!(boot.enter(&mut files));
    assert!(boot.install(&mut nvs, &mut files));
    assert_eq!(boot.page(), Some(Page::Refused(Problem::Refused(Refusal::Storage))));
    assert!(nvs.writes.is_empty() && boot.running().is_none());
}

#[test]
fn a_failed_commit_keeps_the_running_app_and_is_unconfirmed() {
    let app = titled(b"Card");
    // Torn: the old record stands. A lost acknowledgement: the new one does.
    for (put, after) in [(Put::Torn, bundled()), (Put::LostAck, definition(&app))] {
        let mut nvs = Nvs::empty().puts(&[put]);
        let mut boot = Boot::new(&mut nvs);
        let mut files = card(&app);
        assert!(boot.enter(&mut files));
        assert!(boot.install(&mut nvs, &mut files));
        assert_eq!(boot.page(), Some(Page::Unconfirmed(&definition(&app).title[..])), "{put:?}");
        assert_eq!(boot.running(), Some(bundled()), "{put:?}: the running app is unchanged");
        assert_eq!(restart(&nvs), Some(after), "{put:?}");
        assert_eq!(boot.press(Action::Back), Some(Choice::Leave));
        assert!(!boot.enter(&mut files), "Back settles the offer until a restart");
    }
}

#[test]
fn an_unavailable_runtime_takes_the_stored_app_without_a_write() {
    // The record holds the card's app, but the boot read failed.
    let app = titled(b"Card");
    let mut nvs = Nvs::with(Some(record(1, &app)), None).failing_read(0);
    let mut boot = Boot::new(&mut nvs);
    assert_eq!(boot.running(), None);
    let mut files = card(&app);
    assert!(boot.enter(&mut files));
    assert!(boot.install(&mut nvs, &mut files));
    assert_eq!(boot.page(), Some(Page::Installed));
    assert!(nvs.writes.is_empty(), "the stored record is not written again");
    assert_eq!(boot.running(), Some(definition(&app)));
}

#[test]
fn the_offer_settles_only_when_its_page_says_so() {
    let app = titled(b"Card");
    let mut nvs = Nvs::empty();
    let mut boot = Boot::new(&mut nvs);
    let mut files = card(&app);
    assert!(boot.enter(&mut files));
    assert_eq!(boot.press(Action::Next), Some(Choice::Stay));
    assert_eq!(boot.press(Action::Sleep), Some(Choice::Sleep));
    assert_eq!(boot.page(), None, "Sleep drops the offer without settling it");
    assert!(boot.enter(&mut files));
    assert_eq!(boot.press(Action::Back), Some(Choice::Leave));
    assert!(!boot.enter(&mut files) && boot.page().is_none(), "Later settles it");
    assert_eq!(boot.press(Action::Back), None, "the app takes the press");
    // Refusals whose page asks for another try leave the next entry free.
    for (problem, retry) in [(Problem::Refused(Refusal::Changed), true), (Problem::Refused(Refusal::HashFailed), true),
                             (Problem::NoRoom, true), (Problem::Refused(Refusal::Damaged), false),
                             (Problem::Refused(Refusal::Many), false)] {
        let mut prompt = Prompt::Refused(problem);
        assert_eq!(prompt.choose(Action::Activate), Some(Choice::Stay));
        assert_eq!(prompt.choose(Action::Back), Some(Choice::Leave));
        assert_eq!(matches!(prompt, Prompt::Ready), retry, "{problem:?}");
    }
    let mut prompt = Prompt::Installed;
    assert_eq!(prompt.choose(Action::Activate), Some(Choice::Open));
    assert!(matches!(prompt, Prompt::Settled));
    let mut prompt = Prompt::Installed;
    assert_eq!(prompt.choose(Action::Back), Some(Choice::Home));
}

#[test]
fn a_card_that_changed_since_the_offer_writes_nothing_and_can_be_offered_again() {
    let (app, other) = (titled(b"Card"), titled(b"Other"));
    let mut nvs = Nvs::empty();
    let mut boot = Boot::new(&mut nvs);
    let mut files = card(&app);
    assert!(boot.enter(&mut files));
    files.rewrite("app.newt", &other);
    assert!(boot.install(&mut nvs, &mut files));
    assert_eq!(boot.page(), Some(Page::Refused(Problem::Refused(Refusal::Changed))));
    assert!(nvs.writes.is_empty() && boot.running() == Some(bundled()));
    assert_eq!(boot.press(Action::Back), Some(Choice::Leave));
    assert!(boot.enter(&mut files));
    assert_eq!(offered(&boot), definition(&other));
}

#[test]
fn a_card_gone_at_install_is_unreadable_and_writes_nothing() {
    let mut nvs = Nvs::empty();
    let mut boot = Boot::new(&mut nvs);
    assert!(boot.enter(&mut card(&titled(b"Card"))));
    assert_eq!(boot.press(Action::Activate), Some(Choice::Install));
    card_app::unreadable(&mut boot.prompt);
    assert_eq!(boot.page(), Some(Page::Refused(Problem::Refused(Refusal::Unreadable))));
    assert!(nvs.writes.is_empty() && boot.running() == Some(bundled()));
}

#[test]
fn the_running_app_or_a_refused_folder_is_no_offer() {
    let mut nvs = Nvs::empty();
    let mut boot = Boot::new(&mut nvs);
    assert!(!boot.enter(&mut card(BUILTIN_BYTES)), "Pocket Pause is already running");
    let mut boot = Boot::new(&mut nvs);
    assert!(!boot.enter(&mut Card::new()), "an empty folder");
    assert!(matches!(boot.prompt, Prompt::Ready), "nothing to settle");
    let mut two = Card::new().file("a.newt", &titled(b"A")).file("b.newt", &titled(b"B"));
    assert!(boot.enter(&mut two));
    assert_eq!(boot.page(), Some(Page::Refused(Problem::Refused(Refusal::Many))));
}

#[test]
fn no_room_writes_nothing_and_asks_for_another_try() {
    let app = titled(b"Card");
    let mut nvs = Nvs::empty();
    let mut boot = Boot::new(&mut nvs);
    let mut files = card(&app);
    // The offer's wire, then the install's two slot copies. The listing's
    // clone is made first: only the owner's own buffer may be refused.
    let mut listing = files.clone();
    let running = boot.installed.running();
    assert!(refusing(PACKAGE_BYTES, || card_app::enter(&mut boot.prompt, running, &mut listing, &mut files)));
    assert_eq!(boot.page(), Some(Page::Refused(Problem::NoRoom)));
    assert_eq!(boot.press(Action::Back), Some(Choice::Leave));
    assert!(boot.enter(&mut files));
    assert!(refusing(2 * newt_core::app_store::RECORD_BYTES, || boot.install(&mut nvs, &mut files)));
    assert_eq!(boot.page(), Some(Page::Refused(Problem::NoRoom)));
    assert!(nvs.writes.is_empty() && boot.running() == Some(bundled()));
}
