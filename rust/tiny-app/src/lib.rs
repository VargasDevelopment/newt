//! Built-in app codec, routing and rendering (Pocket Pause, Quick Start and
//! the card app offer). MIT; Inter OFL-1.1.
#![no_std]
#![forbid(unsafe_code)]
use newt_core::app::{self, Action, AppState, Screen};
use newt_system_ui::{
    SystemBar, SystemButtons, SystemPage, SYSTEM_CARD, SYSTEM_CONTENT_LEFT as LEFT,
    SYSTEM_CONTENT_WIDTH as WIDTH, SYSTEM_TITLE_TOP as TOP,
};
use newt_ui_canvas::{Frame, UiCanvas};
pub const WIRE_BYTES: usize = 615;
pub const MAX_CARDS: usize = 3;
pub const NO_SELECTION: u32 = 255;
pub const BODY_PIXELS: u32 = 24;
pub const HEADING_PIXELS: u32 = 32;
pub const EXPERIENCE_PIXELS: u32 = 32;
pub const TITLE_PIXELS: u32 = 28;
#[repr(u8)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum CardAction {
    #[default]
    ContinueReading = 1,
    DeviceInfo = 2,
    ShowNote = 3,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub title: [u8; 33],
    pub detail: [u8; 65],
    pub action: CardAction,
}
impl Default for Card {
    fn default() -> Self {
        Self {
            title: [0; 33],
            detail: [0; 65],
            action: CardAction::ContinueReading,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Definition {
    pub id: [u8; 33],
    pub version: [u8; 17],
    pub title: [u8; 33],
    pub subtitle: [u8; 65],
    pub note: [u8; 161],
    pub card_count: u8,
    pub cards: [Card; MAX_CARDS],
}
impl Default for Definition {
    fn default() -> Self {
        Self {
            id: [0; 33],
            version: [0; 17],
            title: [0; 33],
            subtitle: [0; 65],
            note: [0; 161],
            card_count: 0,
            cards: [Card::default(); MAX_CARDS],
        }
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct State {
    pub selected: u32,
    pub note_open: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    None,
    Reading,
    Device,
    Home,
    Sleep,
}
fn field(bytes: &[u8], required: bool) -> bool {
    let Some(end) = bytes.iter().position(|&b| b == 0) else {
        return false;
    };
    (!required || end != 0)
        && bytes[..end].iter().all(|&b| (32..=126).contains(&b))
        && bytes[end..].iter().all(|&b| b == 0)
}
pub(crate) fn cstr(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}
fn slug(value: &[u8]) -> bool {
    value.first().is_some_and(u8::is_ascii_lowercase)
        && value[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}
fn version(value: &[u8]) -> bool {
    let mut parts = value.split(|&b| b == b'.');
    for _ in 0..3 {
        let Some(p) = parts.next() else {
            return false;
        };
        if p.is_empty() || (p.len() > 1 && p[0] == b'0') || !p.iter().all(u8::is_ascii_digit) {
            return false;
        }
    }
    parts.next().is_none()
}
// The installed-app record carries exactly one wire.
const _: () = assert!(WIRE_BYTES == newt_core::app_store::PACKAGE_BYTES);
/// Whether `b` is a complete, canonical NEWTAPP1 wire. It reads only the
/// wire, so a caller can validate without a whole definition on its stack.
pub fn check(b: &[u8; WIRE_BYTES]) -> bool {
    if &b[..8] != b"NEWTAPP1" {
        return false;
    }
    if !field(&b[8..41], true)
        || !slug(cstr(&b[8..41]))
        || !field(&b[41..58], true)
        || !version(cstr(&b[41..58]))
        || !field(&b[58..91], true)
        || !field(&b[91..156], false)
        || !field(&b[156..317], false)
    {
        return false;
    }
    let count = usize::from(b[317]);
    if count == 0 || count > MAX_CARDS {
        return false;
    }
    for i in 0..MAX_CARDS {
        let at = 318 + i * 99;
        if i >= count {
            if b[at..at + 99].iter().any(|&v| v != 0) {
                return false;
            }
        } else if !field(&b[at..at + 33], true)
            || !field(&b[at + 33..at + 98], true)
            || !(1..=3).contains(&b[at + 98])
        {
            return false;
        }
    }
    true
}
/// Validate the complete wire before touching the caller's prior definition.
/// This avoids materializing a second whole definition on the reader stack.
pub fn decode(bytes: Option<&[u8]>, out: &mut Definition) -> bool {
    let Some(b) = bytes.and_then(|b| <&[u8; WIRE_BYTES]>::try_from(b).ok()) else {
        return false;
    };
    if !check(b) {
        return false;
    }
    let count = usize::from(b[317]);
    out.id.copy_from_slice(&b[8..41]);
    out.version.copy_from_slice(&b[41..58]);
    out.title.copy_from_slice(&b[58..91]);
    out.subtitle.copy_from_slice(&b[91..156]);
    out.note.copy_from_slice(&b[156..317]);
    out.card_count = b[317];
    for i in 0..MAX_CARDS {
        let card = &mut out.cards[i];
        let at = 318 + i * 99;
        if i >= count {
            *card = Card::default();
        } else {
            card.title.copy_from_slice(&b[at..at + 33]);
            card.detail.copy_from_slice(&b[at + 33..at + 98]);
            card.action = match b[at + 98] {
                1 => CardAction::ContinueReading,
                2 => CardAction::DeviceInfo,
                _ => CardAction::ShowNote,
            };
        }
    }
    true
}
pub fn dispatch(app: &Definition, state: &mut State, action: Action) -> Intent {
    if action == Action::Sleep {
        return Intent::Sleep;
    }
    if action == Action::Back {
        if state.note_open {
            state.note_open = false;
            return Intent::None;
        }
        return Intent::Home;
    }
    let count = u32::from(app.card_count);
    if count == 0 || count > MAX_CARDS as u32 || state.note_open {
        return Intent::None;
    }
    if state.selected >= count {
        state.selected = 0;
    }
    match action {
        Action::Next => state.selected = (state.selected + 1) % count,
        Action::Previous => state.selected = (state.selected + count - 1) % count,
        Action::Activate => match app.cards[state.selected as usize].action {
            CardAction::ContinueReading => return Intent::Reading,
            CardAction::DeviceInfo => return Intent::Device,
            CardAction::ShowNote => state.note_open = true,
        },
        _ => {}
    }
    Intent::None
}
pub fn apply_intent(state: &mut AppState, intent: Intent) -> bool {
    let before = state.screen;
    match intent {
        Intent::None => return false,
        Intent::Reading => state.screen = Screen::EpubReading,
        Intent::Device => state.screen = Screen::Device,
        Intent::Home => state.screen = Screen::Home,
        Intent::Sleep => return app::dispatch(state, Action::Sleep),
    }
    state.screen != before
}
pub(crate) const BOTTOM: i32 = SYSTEM_CARD.y + SYSTEM_CARD.height - 18;
pub fn draw_note(canvas: &mut UiCanvas<'_>, app: &Definition, selected: u32, mut top: i32) -> i32 {
    let index = if selected < u32::from(app.card_count) {
        selected as usize
    } else {
        0
    };
    let card = app.cards.get(index).unwrap_or(&app.cards[0]);
    let common = app.note[0] != 0;
    top = canvas.fixed_complete_paragraph(
        LEFT,
        top,
        Some(&card.title),
        HEADING_PIXELS,
        WIDTH,
        BOTTOM,
    );
    if top >= 0 {
        top = canvas.fixed_complete_paragraph(
            LEFT,
            top + 16,
            Some(&card.detail),
            if common {
                BODY_PIXELS
            } else {
                EXPERIENCE_PIXELS
            },
            WIDTH,
            BOTTOM,
        );
    }
    if top >= 0 && common {
        top = canvas.fixed_complete_paragraph(
            LEFT,
            top + 16,
            Some(&app.note),
            BODY_PIXELS,
            WIDTH,
            BOTTOM,
        );
    }
    top
}
pub fn render_unavailable(reason: Option<&[u8]>, bar: &SystemBar, frame: Option<&mut Frame>) {
    let Some(frame) = frame else {
        return;
    };
    let mut page = SystemPage::new(
        Some(frame),
        bar,
        SystemButtons {
            back: Some(b"Back"),
            ..Default::default()
        },
        Some(b"A pause for this app."),
    );
    let c = page.canvas();
    c.rect(LEFT, 141, WIDTH, 1, true);
    c.paragraph(
        LEFT,
        174,
        Some(reason.unwrap_or(b"The app could not be loaded.")),
        BODY_PIXELS,
        WIDTH,
        422,
    );
    c.paragraph(
        LEFT,
        462,
        Some(b"Your offline book and device details are still here."),
        BODY_PIXELS,
        WIDTH,
        BOTTOM,
    );
}
pub fn render(app: &Definition, state: &State, bar: &SystemBar, frame: Option<&mut Frame>) {
    let Some(frame) = frame else {
        return;
    };
    if app.card_count == 0 || usize::from(app.card_count) > MAX_CARDS {
        render_unavailable(
            Some(b"This app has no available choices."),
            bar,
            Some(frame),
        );
        return;
    }
    let selected = if state.selected < u32::from(app.card_count) {
        state.selected as usize
    } else {
        0
    };
    let several = app.card_count > 1 && !state.note_open;
    let failed = {
        let mut page = SystemPage::new(
            Some(&mut *frame),
            bar,
            SystemButtons {
                back: Some(b"Back"),
                confirm: (!state.note_open).then_some(b"Select"),
                previous: several.then_some(b"Up"),
                next: several.then_some(b"Down"),
            },
            None,
        );
        let c = page.canvas();
        let mut top = TOP;
        if state.note_open {
            top = draw_note(c, app, selected as u32, top);
        } else {
            let card = &app.cards[selected];
            top = c.fixed_complete_paragraph(LEFT, top, Some(&app.title), TITLE_PIXELS, WIDTH, 204);
            if top >= 0 {
                top = c.fixed_complete_paragraph(
                    LEFT,
                    top + 14,
                    Some(&app.subtitle),
                    BODY_PIXELS,
                    WIDTH,
                    386,
                );
            }
            if top >= 0 {
                c.rect(LEFT, top + 20, WIDTH, 1, true);
                let mut pos = [0; 24];
                pos[..13].copy_from_slice(b"Choice 1 of 1");
                pos[7] = b'1' + selected as u8;
                pos[12] = b'0' + app.card_count;
                c.text(LEFT, top + 40, Some(&pos), 21, false, 0, true);
                top = c.fixed_complete_paragraph(
                    LEFT,
                    top + 84,
                    Some(&card.title),
                    HEADING_PIXELS,
                    WIDTH,
                    554,
                );
                if top >= 0 {
                    top = c.fixed_complete_paragraph(
                        LEFT,
                        top + 14,
                        Some(&card.detail),
                        BODY_PIXELS,
                        WIDTH,
                        BOTTOM,
                    );
                }
            }
        }
        top < 0
    };
    if failed {
        render_unavailable(
            Some(b"This app's text does not fit at a readable size."),
            bar,
            Some(frame),
        );
    }
}

// The card app offer and its outcome pages (firmware app_runtime/card_app.rs).
pub mod install;
pub mod pause;

// #272 renderer; caller lends the existing logical/native scratch frames.
pub mod quick_start;
