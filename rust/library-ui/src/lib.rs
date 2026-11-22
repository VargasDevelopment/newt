//! Books shelf UI over `newt_core::library_catalog`. MIT; Inter OFL-1.1.
#![no_std]
#![forbid(unsafe_code)]
use core::fmt::{self, Write};
pub use newt_core::{app::Action, frame::Frame, library_catalog::LibraryCatalog};
use newt_core::{
    library_catalog::{LibraryStatus, library_base_name},
    library_name::FILENAME_BYTES,
};
pub use newt_system_ui::SystemBar;
use newt_system_ui::{
    SystemButtons, SystemPage, clamped_list_buttons, draw_frosted_system_header, draw_system_footer,
};
use newt_ui_canvas::{HomePlane, UiCanvas, text_width};
pub const SHELF_BOOKS: usize = 4;
pub const COVER_WIDTH: i32 = 144;
pub const COVER_HEIGHT: i32 = 188;
pub const COVER_ROW_BYTES: usize = 18;
/// A scanline is borrowed for each call. End runs even when begin fails.
pub trait LibraryCoverSource {
    fn begin(&mut self, filename: Option<&[u8]>) -> bool;
    fn read_row(&mut self, row: &mut [u8; COVER_ROW_BYTES]) -> bool;
    fn end(&mut self);
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct LibraryUiState<'a> {
    pub selected: usize,
    pub message: Option<&'a [u8]>,
    pub loading: bool,
    pub no_card: bool,
    pub painted_focus: u8,
    pub focus_pending: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LibraryIntent {
    None,
    Repaint,
    Open,
    Home,
    Sleep,
    OpenSample,
}
pub fn shelf_first(selected: usize) -> usize {
    selected / SHELF_BOOKS * SHELF_BOOKS
}
pub fn invalidate_frame(state: &mut LibraryUiState<'_>) {
    state.painted_focus = 0;
    state.focus_pending = false;
}
pub fn dispatch(state: &mut LibraryUiState<'_>, count: usize, action: Action) -> LibraryIntent {
    if action != Action::Next && action != Action::Previous {
        invalidate_frame(state);
    }
    if action == Action::Back {
        return LibraryIntent::Home;
    }
    if action == Action::Sleep {
        return LibraryIntent::Sleep;
    }
    if state.loading {
        return LibraryIntent::None;
    }
    if state.no_card {
        return if action == Action::Activate {
            LibraryIntent::OpenSample
        } else {
            LibraryIntent::None
        };
    }
    if count == 0 {
        return LibraryIntent::None;
    }
    if state.selected >= count {
        invalidate_frame(state);
    }
    state.selected = state.selected.min(count - 1);
    if action == Action::Activate {
        return LibraryIntent::Open;
    }
    let previous = state.selected;
    if action == Action::Previous && state.selected > 0 {
        state.selected -= 1;
    }
    if action == Action::Next && state.selected + 1 < count {
        state.selected += 1;
    }
    if previous != state.selected {
        if state.message.is_some() || shelf_first(previous) != shelf_first(state.selected) {
            invalidate_frame(state);
        }
        state.focus_pending = state.painted_focus > 0 && state.painted_focus <= SHELF_BOOKS as u8;
        state.message = None;
        return LibraryIntent::Repaint;
    }
    LibraryIntent::None
}
fn display_name(filename: Option<&[u8]>) -> [u8; FILENAME_BYTES] {
    let raw = library_base_name(filename).unwrap_or(b"Book unavailable");
    let mut title = [0; FILENAME_BYTES];
    let n = raw.len().min(FILENAME_BYTES - 1);
    title[..n].copy_from_slice(&raw[..n]);
    if n >= 5 {
        title[n - 5] = 0;
    }
    title
}
fn origin(slot: usize) -> (i32, i32) {
    (74 + (slot % 2) as i32 * 236, 138 + (slot / 2) as i32 * 232)
}
fn draw_cover(
    c: &mut UiCanvas<'_>,
    x: i32,
    y: i32,
    filename: Option<&[u8]>,
    source: Option<&mut dyn LibraryCoverSource>,
) -> bool {
    let Some(source) = source else {
        return false;
    };
    let mut complete = source.begin(filename);
    let mut row = [0; COVER_ROW_BYTES];
    for yy in 0..COVER_HEIGHT {
        if !complete {
            break;
        }
        complete = source.read_row(&mut row);
        if complete {
            for xx in 0..COVER_WIDTH {
                c.pixel(
                    x + xx,
                    y + yy,
                    row[xx as usize / 8] & (0x80 >> (xx % 8)) != 0,
                );
            }
        }
    }
    source.end();
    complete
}
fn cover_focus(c: &mut UiCanvas<'_>, title: &[u8], slot: usize, focused: bool) {
    let (x, y) = origin(slot);
    if focused {
        for inset in 0..3 {
            c.outline(
                x - 5 + inset,
                y - 5 + inset,
                COVER_WIDTH + 10 - 2 * inset,
                COVER_HEIGHT + 10 - 2 * inset,
            );
        }
        for dy in -7i32..=7 {
            c.rect(
                x - 24,
                y + COVER_HEIGHT / 2 + dy,
                8 - 7.min(dy.abs()),
                1,
                true,
            );
        }
    }
    c.text(x - 24, y + 196, Some(title), 21, focused, 192, true);
}
fn cover_slot(
    c: &mut UiCanvas<'_>,
    filename: Option<&[u8]>,
    title: &[u8],
    slot: usize,
    focused: bool,
    source: Option<&mut dyn LibraryCoverSource>,
) {
    let (x, y) = origin(slot);
    if !draw_cover(c, x, y, filename, source) {
        c.panel(x, y, COVER_WIDTH, COVER_HEIGHT);
        c.text(x + 16, y + 32, Some(title), 21, false, 112, true);
        c.text(x + 16, y + 139, Some(b"No cover"), 21, false, 112, true);
    }
    cover_focus(c, title, slot, focused);
}
struct Counter {
    bytes: [u8; 48],
    used: usize,
}
impl Write for Counter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.used + s.len() >= self.bytes.len() {
            return Err(fmt::Error);
        }
        self.bytes[self.used..self.used + s.len()].copy_from_slice(s.as_bytes());
        self.used += s.len();
        Ok(())
    }
}
fn shelf_counter(c: &mut UiCanvas<'_>, selected: usize, count: usize) {
    c.rect(298, 90, 180, 32, false);
    let mut label = Counter {
        bytes: [0; 48],
        used: 0,
    };
    let _ = write!(label, "{} of {}", selected + 1, count);
    c.text(
        478 - text_width(Some(&label.bytes), 21, false),
        95,
        Some(&label.bytes),
        21,
        false,
        180,
        true,
    );
}
fn buttons(catalog: Option<&LibraryCatalog>, state: &LibraryUiState<'_>) -> SystemButtons<'static> {
    let count = catalog
        .filter(|c| !state.loading && !state.no_card && c.status() == LibraryStatus::Ok)
        .map_or(0, LibraryCatalog::size);
    let selected = if count > 0 {
        state.selected.min(count - 1)
    } else {
        0
    };
    clamped_list_buttons(
        Some(b"Back"),
        if !state.loading && (state.no_card || count > 0) {
            Some(b"Open")
        } else {
            None
        },
        selected as u32,
        count as u32,
    )
}
fn note<'a>(catalog: Option<&LibraryCatalog>, state: &LibraryUiState<'a>) -> Option<&'a [u8]> {
    if state.message.is_some() || state.loading || state.no_card {
        state.message
    } else {
        catalog.and_then(LibraryCatalog::notice).map(str::as_bytes)
    }
}
fn selected_title(c: &mut UiCanvas<'_>, filename: Option<&[u8]>, bottom: i32) {
    let mut title = display_name(filename);
    if c.fixed_complete_paragraph(50, 616, Some(&title), 24, 428, bottom) >= 0 {
        return;
    }
    if c.fixed_complete_paragraph(50, 616, Some(&title), 21, 428, bottom) >= 0 {
        return;
    }
    let mut length = title.iter().position(|&b| b == 0).unwrap_or(title.len());
    while length > 0 {
        length -= 1;
        while length > 0 && title[length] & 0xc0 == 0x80 {
            length -= 1;
        }
        title[length] = 0;
        if length + 4 > title.len() {
            continue;
        }
        title[length..length + 4].copy_from_slice(b"\xe2\x80\xa6\0");
        if c.fixed_complete_paragraph(50, 616, Some(&title), 21, 428, bottom) >= 0 {
            return;
        }
    }
}
pub fn render(
    catalog: Option<&LibraryCatalog>,
    state: &LibraryUiState<'_>,
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    mut covers: Option<&mut dyn LibraryCoverSource>,
) {
    let Some(frame) = frame else {
        return;
    };
    let mut page = SystemPage::new(Some(frame), bar, buttons(catalog, state), Some(b"Library"));
    let c = page.canvas();
    let count = catalog.map_or(0, LibraryCatalog::size);
    if state.loading {
        c.text(50, 170, Some(b"Loading your library"), 28, true, 428, true);
        c.paragraph(
            50,
            224,
            Some(b"Looking for books on your spare card."),
            24,
            428,
            370,
        );
    } else if state.no_card {
        c.text(50, 170, Some(b"No SD card"), 28, true, 428, true);
        c.paragraph(
            50,
            224,
            Some(b"Insert the card to see your books."),
            24,
            428,
            370,
        );
        c.paragraph(
            50,
            417,
            Some(b"Open reads the built-in sample instead."),
            21,
            428,
            570,
        );
    } else if catalog.is_none_or(|c| c.status() != LibraryStatus::Ok) || count == 0 {
        let empty = catalog.is_some_and(|c| c.status() == LibraryStatus::Ok);
        c.text(
            50,
            170,
            Some(if empty {
                b"Your shelf is empty"
            } else {
                b"Books are unavailable"
            }),
            28,
            true,
            428,
            true,
        );
        c.paragraph(
            50,
            224,
            Some(if empty {
                b"Add EPUB books anywhere on your card."
            } else {
                b"The card could not be read."
            }),
            24,
            428,
            370,
        );
        c.paragraph(
            50,
            417,
            Some(if empty {
                b"Back returns to Home."
            } else {
                b"Original books have not been changed."
            }),
            21,
            428,
            570,
        );
    } else if let Some(catalog) = catalog {
        let selected = state.selected.min(count - 1);
        shelf_counter(c, selected, count);
        let first = shelf_first(selected);
        for slot in 0..SHELF_BOOKS.min(count - first) {
            let filename = catalog.filename(first + slot);
            let title = display_name(filename);
            // Reborrow the single caller-owned source; never retain a handle.
            let source = match covers {
                Some(ref mut source) => Some(&mut **source as &mut dyn LibraryCoverSource),
                None => None,
            };
            cover_slot(c, filename, &title, slot, first + slot == selected, source);
        }
        c.rect(50, 604, 428, 1, true);
        selected_title(
            c,
            catalog.filename(selected),
            if note(Some(catalog), state).is_some() {
                673
            } else {
                704
            },
        );
    }
    if let Some(note) = note(catalog, state) {
        c.text(50, 679, Some(note), 18, false, 428, true);
    }
}
fn clear_focus(c: &mut UiCanvas<'_>, slot: usize) {
    let (x, y) = origin(slot);
    c.rect(x - 5, y - 5, COVER_WIDTH + 10, 5, false);
    c.rect(x - 5, y + COVER_HEIGHT, COVER_WIDTH + 10, 5, false);
    c.rect(x - 5, y, 5, COVER_HEIGHT, false);
    c.rect(x + COVER_WIDTH, y, 5, COVER_HEIGHT, false);
    c.rect(x - 24, y + COVER_HEIGHT / 2 - 7, 8, 15, false);
    c.rect(x - 24, y + 196, 192, 30, false);
}
#[inline(never)]
fn paint_focus(
    catalog: &LibraryCatalog,
    state: &LibraryUiState<'_>,
    frame: &mut Frame,
    bar: &SystemBar,
) {
    draw_frosted_system_header(bar, HomePlane::Monochrome, Some(frame));
    {
        let mut c = UiCanvas::portrait(Some(frame));
        let slot = state.selected % SHELF_BOOKS;
        let first = shelf_first(state.selected);
        for focus in [usize::from(state.painted_focus - 1), slot] {
            clear_focus(&mut c, focus);
            cover_focus(
                &mut c,
                &display_name(catalog.filename(first + focus)),
                focus,
                focus == slot,
            );
        }
        shelf_counter(&mut c, state.selected, catalog.size());
        c.rect(50, 616, 428, 89, false);
        let note = note(Some(catalog), state);
        selected_title(
            &mut c,
            catalog.filename(state.selected),
            if note.is_some() { 673 } else { 704 },
        );
        if let Some(note) = note {
            c.text(50, 679, Some(note), 18, false, 428, true);
        }
    }
    draw_system_footer(
        Some(frame),
        buttons(Some(catalog), state),
        HomePlane::Monochrome,
    );
}
/// Returns true when the full paint path ran, including blank/error shelves.
pub fn paint(
    catalog: Option<&LibraryCatalog>,
    state: &mut LibraryUiState<'_>,
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    covers: Option<&mut dyn LibraryCoverSource>,
) -> bool {
    let Some(frame) = frame else {
        invalidate_frame(state);
        return false;
    };
    let shelf = catalog.is_some_and(|c| {
        c.status() == LibraryStatus::Ok && c.size() > 0 && state.selected < c.size()
    }) && !state.loading
        && !state.no_card
        && state.message.is_none();
    let focus = shelf
        && state.focus_pending
        && state.painted_focus > 0
        && state.painted_focus <= SHELF_BOOKS as u8;
    if focus {
        if let Some(catalog) = catalog {
            paint_focus(catalog, state, frame, bar);
        }
    } else {
        render(catalog, state, Some(frame), bar, covers);
    }
    state.painted_focus = if shelf {
        (state.selected % SHELF_BOOKS + 1) as u8
    } else {
        0
    };
    state.focus_pending = false;
    !focus
}
