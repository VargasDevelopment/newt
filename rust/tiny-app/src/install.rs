// SPDX-License-Identifier: MIT
//! The card app offer and its outcome pages, shared by firmware and preview.
//! The firmware owns the offer, the record and the decision of what to show;
//! this only maps a page and a press to a choice and draws the page.
use crate::{BODY_PIXELS, CardAction, Definition, TITLE_PIXELS, cstr};
use newt_core::{app::Action, app_store::Refusal};
use newt_system_ui::{
    SYSTEM_CONTENT_LEFT as LEFT, SYSTEM_CONTENT_WIDTH as WIDTH, SystemBar, SystemButtons,
    SystemPage, SYSTEM_BODY_TOP as BODY_TOP,
};
use newt_ui_canvas::{Frame, UiCanvas};

const BOTTOM: i32 = crate::BOTTOM;

/// Why nothing was installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Problem {
    Refused(Refusal),
    /// The heap could not lend the offer or install buffers.
    NoRoom,
}

impl Problem {
    /// Its copy asks the reader to try again or to fix the folder, so leaving
    /// must not settle the offer until the next restart.
    pub fn retry(self) -> bool {
        matches!(self, Self::NoRoom | Self::Refused(Refusal::Changed | Refusal::HashFailed | Refusal::Many))
    }

    fn reason(self) -> &'static [u8] {
        match self {
            Self::Refused(Refusal::Damaged) => b"The file is damaged or isn't a Newt app.",
            Self::Refused(Refusal::Newer) => b"This app needs a newer Newt.",
            Self::Refused(Refusal::Many) => {
                b"There is more than one app in Newt/Apps. Leave just one there."
            }
            Self::Refused(Refusal::Unreadable) => b"Newt could not read the card.",
            Self::Refused(Refusal::Changed) => b"The app file changed. Open Apps again to see it.",
            Self::Refused(Refusal::HashFailed) => {
                b"Newt could not check this app right now. Try again from Home."
            }
            Self::Refused(Refusal::Storage) => b"Newt's app storage needs attention.",
            Self::NoRoom => b"Newt doesn't have room to check this app right now. Try again from Home.",
        }
    }
}

/// One page of the card app flow. Installed names no app: after activation
/// the running app is the new one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Page<'a> {
    Offer(&'a Definition),
    Installed,
    Refused(Problem),
    /// Carries the offered app's title; the running app is still the old one.
    Unconfirmed(&'a [u8]),
}

/// What a press on a page asks the owner to do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Choice {
    Stay,
    Install,
    /// Later, or Back after a problem: show the running app.
    Leave,
    Open,
    Home,
    Sleep,
}

pub fn dispatch(page: &Page<'_>, action: Action) -> Choice {
    match (page, action) {
        (_, Action::Sleep) => Choice::Sleep,
        (Page::Offer(_), Action::Back) => Choice::Leave,
        (Page::Offer(_), Action::Activate) => Choice::Install,
        (Page::Installed, Action::Back) => Choice::Home,
        (Page::Installed, Action::Activate) => Choice::Open,
        (Page::Refused(_) | Page::Unconfirmed(_), Action::Back) => Choice::Leave,
        _ => Choice::Stay,
    }
}

/// `running` is the app Apps shows now, None while it is unavailable.
pub fn render(page: &Page<'_>, running: Option<&Definition>, bar: &SystemBar, frame: Option<&mut Frame>) {
    let Some(frame) = frame else {
        return;
    };
    let (title, back, confirm): (&[u8], &[u8], Option<&[u8]>) = match page {
        Page::Offer(_) => (b"An app on your card", b"Later", Some(b"Install")),
        Page::Installed => (b"Installed", b"Home", Some(b"Open")),
        Page::Refused(_) => (b"Can't install this app", b"Back", None),
        Page::Unconfirmed(_) => (b"Install not confirmed", b"Back", None),
    };
    let buttons = SystemButtons { back: Some(back), confirm, ..Default::default() };
    let mut page_frame = SystemPage::new(Some(frame), bar, buttons, Some(title));
    let c = page_frame.canvas();
    let current = running.map(|app| cstr(&app.title));
    match *page {
        Page::Offer(offered) => offer(c, offered, current),
        Page::Installed => {
            let name = current.unwrap_or_default();
            let top = body(c, BODY_TOP, &Text::new().push(name).push(b" is in Apps now."));
            body(c, top + 16, b"You can delete the file from your card.");
        }
        Page::Refused(problem) => {
            let top = body(c, BODY_TOP, problem.reason());
            let unchanged = match current {
                Some(name) => Text::new().push(b"Nothing changed. ").push(name).push(b" is still in Apps."),
                None => Text::new().push(b"Nothing changed."),
            };
            body(c, top + 16, &unchanged);
        }
        Page::Unconfirmed(offered) => {
            let line = Text::new().push(b"Newt could not confirm the install. After a restart, Apps shows ");
            let line = match current {
                Some(name) => line.push(b"either ").push(name).push(b" or ").push(cstr(offered)).push(b"."),
                None => line.push(cstr(offered)).push(b", an app saved earlier, or the message it shows now."),
            };
            body(c, BODY_TOP, &line);
        }
    }
}

/// A body paragraph; the next paragraph's top, negative once one did not fit.
fn body(c: &mut UiCanvas<'_>, top: i32, text: &[u8]) -> i32 {
    if top < 0 {
        return top;
    }
    c.fixed_complete_paragraph(LEFT, top, Some(text), BODY_PIXELS, WIDTH, BOTTOM)
}

fn offer(c: &mut UiCanvas<'_>, app: &Definition, current: Option<&[u8]>) {
    let mut top = c.fixed_complete_paragraph(LEFT, BODY_TOP, Some(&app.title), TITLE_PIXELS, WIDTH, BOTTOM);
    if top >= 0 && app.subtitle[0] != 0 {
        top = body(c, top + 10, &app.subtitle);
    }
    if top < 0 {
        return;
    }
    c.rect(LEFT, top + 20, WIDTH, 1, true);
    let choices = Text::new()
        .push(b"Version ")
        .push(cstr(&app.version))
        .push(" · ".as_bytes())
        .push(&[b'0' + app.card_count])
        .push(if app.card_count == 1 { b" choice" } else { b" choices" });
    c.text(LEFT, top + 40, Some(&choices), 21, false, 0, true);
    let replaces = match current {
        Some(name) => Text::new().push(b"It will replace ").push(name).push(b" in Apps."),
        None => Text::new().push(b"It will be the app in Apps."),
    };
    top = body(c, top + 84, &replaces);
    body(c, top + 16, &capabilities(app));
}

/// "It works offline. It can ..." from the card actions this app uses, in
/// the actions' own order. Each phrase names what that action can reach.
fn capabilities(app: &Definition) -> Text {
    const PHRASES: [(CardAction, &[u8]); 3] = [
        (CardAction::ContinueReading, b"open your book"),
        (CardAction::DeviceInfo, b"open device details"),
        (CardAction::ShowNote, b"show its notes"),
    ];
    let count = usize::from(app.card_count).min(app.cards.len());
    let used = PHRASES.map(|(action, phrase)| {
        app.cards[..count].iter().any(|card| card.action == action).then_some(phrase)
    });
    let total = used.iter().flatten().count();
    let mut text = Text::new().push(b"It works offline. It can ");
    for (index, phrase) in used.iter().flatten().enumerate() {
        text = match index {
            0 => text,
            _ if index + 1 == total => text.push(b" and "),
            _ => text.push(b", "),
        }
        .push(phrase);
    }
    text.push(b".")
}

/// A short composed line on the stack; longer input is cut, never wrapped.
struct Text {
    bytes: [u8; 160],
    len: usize,
}

impl Text {
    fn new() -> Self {
        Self { bytes: [0; 160], len: 0 }
    }

    fn push(mut self, part: &[u8]) -> Self {
        let n = part.len().min(self.bytes.len() - 1 - self.len);
        self.bytes[self.len..self.len + n].copy_from_slice(&part[..n]);
        self.len += n;
        self
    }
}

impl core::ops::Deref for Text {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
