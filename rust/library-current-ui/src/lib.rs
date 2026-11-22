//! Books shelf UI over `newt-library-current`. MIT; Inter OFL-1.1.
#![no_std]
#![forbid(unsafe_code)]
use core::fmt::{self, Write};
pub use newt_core::{app::Action, frame::Frame};
pub use newt_library_current::{Catalog, LibraryStatus};
use newt_library_current::{library_base_name, library_name::FILENAME_BYTES, list::Files};
pub use newt_system_ui::SystemBar;
use newt_system_ui::{
    SystemButtons, SystemPage, clamped_list_buttons, draw_frosted_system_header, draw_system_footer,
};
use newt_ui_canvas::{HomePlane, UiCanvas, text_width};
pub const SHELF_BOOKS: usize = 4;
pub const COVER_WIDTH: i32 = 144;
pub const COVER_HEIGHT: i32 = 188;
pub const COVER_ROW_BYTES: usize = 18;
/// A scanline and file view are borrowed only for one synchronous call.
/// Sources retain handles, never Files/frame/scratch loans. No callback may
/// reenter this UI. End runs even when begin fails (including a null name).
pub trait LibraryCoverSource {
    fn begin(&mut self, files: &mut dyn Files, filename: Option<&[u8]>) -> bool;
    fn read_row(&mut self, files: &mut dyn Files, row: &mut [u8; COVER_ROW_BYTES]) -> bool;
    fn end(&mut self, files: &mut dyn Files);
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
    let n = raw.iter().position(|&b| b == 0).unwrap_or(raw.len()).min(FILENAME_BYTES - 1);
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
    frame: &mut Frame,
    files: &mut dyn Files,
    x: i32,
    y: i32,
    filename: Option<&[u8]>,
    source: Option<&mut dyn LibraryCoverSource>,
) -> bool {
    let Some(source) = source else {
        return false;
    };
    let mut complete = source.begin(files, filename);
    let mut row = [0; COVER_ROW_BYTES];
    for yy in 0..COVER_HEIGHT {
        if !complete {
            break;
        }
        complete = source.read_row(files, &mut row);
        if complete {
            let mut c = shelf_canvas(frame);
            for xx in 0..COVER_WIDTH {
                c.pixel(
                    x + xx,
                    y + yy,
                    row[xx as usize / 8] & (0x80 >> (xx % 8)) != 0,
                );
            }
        }
    }
    source.end(files);
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
    frame: &mut Frame,
    files: &mut dyn Files,
    filename: Option<&[u8]>,
    title: &[u8],
    slot: usize,
    focused: bool,
    source: Option<&mut dyn LibraryCoverSource>,
) {
    let (x, y) = origin(slot);
    let complete = draw_cover(frame, files, x, y, filename, source);
    let c = &mut shelf_canvas(frame);
    if !complete {
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
fn buttons(catalog: Option<&Catalog>, state: &LibraryUiState<'_>) -> SystemButtons<'static> {
    let count = catalog
        .filter(|c| !state.loading && !state.no_card && c.status() == LibraryStatus::Ok)
        .map_or(0, Catalog::size);
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
fn note<'a>(catalog: Option<&Catalog>, state: &LibraryUiState<'a>) -> Option<&'a [u8]> {
    if state.message.is_some() || state.loading || state.no_card {
        state.message
    } else {
        catalog.and_then(Catalog::notice).map(str::as_bytes)
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
    mut catalog: Option<&mut Catalog>,
    files: &mut dyn Files,
    state: &LibraryUiState<'_>,
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    mut covers: Option<&mut dyn LibraryCoverSource>,
) {
    let Some(frame) = frame else {
        return;
    };
    if status::render_status(catalog.as_deref(), state, frame, bar) { return; }
    // Release the page's canvas before the first filesystem or cover callback.
    drop(SystemPage::new(Some(frame), bar, buttons(catalog.as_deref(), state), Some(b"Books")));
    let count = catalog.as_deref().map_or(0, Catalog::size);
    if let Some(catalog) = catalog.as_deref_mut() {
        let selected = state.selected.min(count - 1);
        shelf_counter(&mut shelf_canvas(frame), selected, count);
        let first = shelf_first(selected);
        for slot in 0..SHELF_BOOKS.min(count - first) {
            // Ask twice, including after a failed refill; do not coalesce.
            let title = display_name(catalog.filename(files, first + slot));
            let filename = catalog.filename(files, first + slot);
            let source = match covers {
                Some(ref mut source) => Some(&mut **source as &mut dyn LibraryCoverSource),
                None => None,
            };
            cover_slot(frame, files, filename, &title, slot, first + slot == selected, source);
        }
        shelf_canvas(frame).rect(50, 604, 428, 1, true);
        let bottom = if note(Some(catalog), state).is_some() { 673 } else { 704 };
        let filename = catalog.filename(files, selected);
        selected_title(&mut shelf_canvas(frame), filename, bottom);
    }
    if let Some(note) = note(catalog.as_deref(), state) {
        shelf_canvas(frame).text(50, 679, Some(note), 18, false, 428, true);
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
    catalog: &mut Catalog,
    files: &mut dyn Files,
    state: &LibraryUiState<'_>,
    frame: &mut Frame,
    bar: &SystemBar,
) {
    draw_frosted_system_header(bar, HomePlane::Monochrome, Some(frame));
    let slot = state.selected % SHELF_BOOKS;
    let first = shelf_first(state.selected);
    for focus in [usize::from(state.painted_focus - 1), slot] {
        clear_focus(&mut UiCanvas::portrait(Some(frame)), focus);
        let title = display_name(catalog.filename(files, first + focus));
        cover_focus(&mut UiCanvas::portrait(Some(frame)), &title, focus, focus == slot);
    }
    shelf_counter(&mut UiCanvas::portrait(Some(frame)), state.selected, catalog.size());
    UiCanvas::portrait(Some(frame)).rect(50, 616, 428, 89, false);
    let note = note(Some(catalog), state);
    let filename = catalog.filename(files, state.selected);
    selected_title(&mut UiCanvas::portrait(Some(frame)), filename, if note.is_some() {673} else {704});
    if let Some(note) = note {
        UiCanvas::portrait(Some(frame)).text(50, 679, Some(note), 18, false, 428, true);
    }
    draw_system_footer(Some(frame), buttons(Some(catalog), state), HomePlane::Monochrome);
}
/// Returns true when the full paint path ran, including blank/error shelves.
pub fn paint(
    mut catalog: Option<&mut Catalog>,
    files: &mut dyn Files,
    state: &mut LibraryUiState<'_>,
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    covers: Option<&mut dyn LibraryCoverSource>,
) -> bool {
    let Some(frame) = frame else {
        invalidate_frame(state);
        return false;
    };
    let shelf = catalog.as_deref().is_some_and(|c| {
        c.status() == LibraryStatus::Ok && c.size() > 0 && state.selected < c.size()
    }) && !state.loading
        && !state.no_card
        && state.message.is_none();
    let focus = shelf
        && state.focus_pending
        && state.painted_focus > 0
        && state.painted_focus <= SHELF_BOOKS as u8;
    if focus {
        if let Some(catalog) = catalog.as_deref_mut() {
            paint_focus(catalog, files, state, frame, bar);
        }
    } else {
        render(catalog, files, state, Some(frame), bar, covers);
    }
    state.painted_focus = if shelf {
        (state.selected % SHELF_BOOKS + 1) as u8
    } else {
        0
    };
    state.focus_pending = false;
    !focus
}

// Recreate only the lightweight view; no framebuffer or title cache is owned.
fn shelf_canvas(frame: &mut Frame) -> UiCanvas<'_> {
    let mut c = UiCanvas::portrait(Some(frame));
    let card = newt_system_ui::SYSTEM_CARD;
    c.clip(card.x, card.y, card.width, card.height);
    c
}

mod borrowed;
pub use borrowed::alpha15_library_ui_borrowed;

mod status;
/// Compose the actual no-card/loading/unavailable shelf without manufacturing
/// a filesystem capability before the card has mounted.
pub fn paint_without_files(state: &mut LibraryUiState<'_>, frame: Option<&mut Frame>, bar: &SystemBar) -> bool {
    let Some(frame) = frame else { invalidate_frame(state); return false; };
    let full = status::render_status(None, state, frame, bar);
    state.painted_focus = 0;
    state.focus_pending = false;
    full
}
