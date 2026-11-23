// SPDX-License-Identifier: MIT
//! Offers the one app file waiting in /Newt/Apps when Apps opens, and installs
//! it only after Install. Startup never reads the card for apps, and nothing
//! is written before Install. The offer's heap is held only while its page is
//! shown; the install's slot copies only for the install itself.
use super::{installed_app::InstalledApp, package::X3Payload};
use newt_core::{
    app::Action,
    app_card::{self, Candidate, Scan},
    app_store::{self, Install, PACKAGE_BYTES, RECORD_BYTES, Refusal},
    crossink_import::Files as Listing,
    durable::Backend,
    update_card::Files,
};
use newt_sdk_heap::Fixed;
use newt_tiny_app::{
    Definition, State, decode,
    install::{Choice, Page, Problem},
};

/// The offered app, the digest that names it and where it was read.
pub struct Found {
    definition: Definition,
    digest: [u8; 32],
    candidate: Candidate,
}

/// Where the card app flow stands this boot. Only RAM: a restart forgets a
/// declined offer, and the record alone says which app runs.
#[derive(Default)]
pub enum Prompt {
    /// The next Apps entry reads the card.
    #[default]
    Ready,
    /// Declined or done: Apps opens as usual until the next restart.
    Settled,
    Offer(Fixed<Found>),
    Installed,
    Refused(Problem),
    /// The offered title, kept for the page after its offer was dropped.
    Unconfirmed(Fixed<[u8; 33]>),
}

impl Prompt {
    pub fn page(&self) -> Option<Page<'_>> {
        Some(match self {
            Self::Ready | Self::Settled => return None,
            Self::Offer(found) => Page::Offer(&found.definition),
            Self::Installed => Page::Installed,
            Self::Refused(problem) => Page::Refused(*problem),
            Self::Unconfirmed(title) => Page::Unconfirmed(&title[..]),
        })
    }

    /// A press while a page is shown; None when no page is. Sleep frees the
    /// page; the wake is a restart, which remembers nothing of it.
    pub fn choose(&mut self, action: Action) -> Option<Choice> {
        let choice = newt_tiny_app::install::dispatch(&self.page()?, action);
        match choice {
            Choice::Stay | Choice::Install => {}
            Choice::Sleep => *self = Self::Ready,
            Choice::Leave if matches!(self, Self::Refused(problem) if problem.retry()) => *self = Self::Ready,
            Choice::Leave | Choice::Open | Choice::Home => *self = Self::Settled,
        }
        Some(choice)
    }
}

/// Entering Apps: scan once and judge the one file. True when a page is shown.
pub fn enter(prompt: &mut Prompt, running: Option<&Definition>, listing: &mut impl Listing, files: &mut dyn Files) -> bool {
    if !matches!(prompt, Prompt::Ready) {
        return false;
    }
    *prompt = match app_card::scan(listing) {
        Scan::Nothing => return false,
        Scan::Refused(refusal) => Prompt::Refused(Problem::Refused(refusal)),
        Scan::One(candidate) => offer(running, files, candidate),
    };
    !matches!(prompt, Prompt::Settled)
}

/// The same app as the running one is no offer.
fn offer(running: Option<&Definition>, files: &mut dyn Files, candidate: Candidate) -> Prompt {
    let Some(mut wire) = Fixed::try_new_with(|| [0; PACKAGE_BYTES]) else {
        return Prompt::Refused(Problem::NoRoom);
    };
    let mut digest = [0; 32];
    if let Err(refusal) = app_card::offer(files, &mut X3Payload, &candidate, &mut wire, &mut digest) {
        return Prompt::Refused(Problem::Refused(refusal));
    }
    let Some(mut found) = Fixed::try_new_with(|| Found { definition: Definition::default(), digest, candidate }) else {
        return Prompt::Refused(Problem::NoRoom);
    };
    // offer() already ran the decoder's check, so this cannot refuse.
    if !decode(Some(&wire[..]), &mut found.definition) {
        return Prompt::Refused(Problem::Refused(Refusal::Damaged));
    }
    if running == Some(&found.definition) { Prompt::Settled } else { Prompt::Offer(found) }
}

/// Install after the reader chose it. The card is read again inside the
/// record session, so a file that changed since the offer is refused. True
/// when the prompt changed.
pub fn install(
    prompt: &mut Prompt,
    installed: &mut InstalledApp,
    tiny: &mut State,
    store: &mut impl Backend,
    files: &mut dyn Files,
) -> bool {
    let Prompt::Offer(found) = prompt else {
        return false;
    };
    let (digest, candidate) = (found.digest, found.candidate);
    // Allocated first, so the Unconfirmed page can always name the app.
    let Some(title) = Fixed::try_new_with(|| found.definition.title) else {
        *prompt = Prompt::Refused(Problem::NoRoom);
        return true;
    };
    // The offer is freed before the slot copies are taken.
    *prompt = Prompt::Ready;
    let Some(mut slots) = Fixed::try_new_with(|| [[0; RECORD_BYTES]; 2]) else {
        *prompt = Prompt::Refused(Problem::NoRoom);
        return true;
    };
    let mut activated = false;
    let outcome = app_store::install(
        store,
        &mut X3Payload,
        &mut slots,
        &digest,
        |wire| app_card::read(files, &candidate, wire),
        |selected| activated = installed.activate(selected),
    );
    *prompt = match outcome {
        Install::Installed if activated => {
            *tiny = State::default();
            Prompt::Installed
        }
        // Durable but not running: after a restart either app may load.
        Install::Installed | Install::Unconfirmed => Prompt::Unconfirmed(title),
        Install::Refused(refusal) => Prompt::Refused(Problem::Refused(refusal)),
    };
    true
}

/// Install was chosen but the card could not be reached at all.
pub fn unreadable(prompt: &mut Prompt) {
    if matches!(prompt, Prompt::Offer(_)) {
        *prompt = Prompt::Refused(Problem::Refused(Refusal::Unreadable));
    }
}
