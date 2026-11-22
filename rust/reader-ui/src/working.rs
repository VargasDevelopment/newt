use crate::*;
use newt_system_ui::{SystemBar, SystemButtons, SystemPage};
#[derive(Clone, Copy)]
pub enum SlowWork {
    FirstOpen,
    Opening,
    Saving,
    Applying,
    Loading,
}
pub fn render_book_opening_ui(bar: &SystemBar, frame: Option<&mut Frame>) {
    if frame.is_none() {
        return;
    }
    let mut page = SystemPage::new(
        frame,
        bar,
        SystemButtons {
            back: Some(b"Cancel"),
            ..SystemButtons::default()
        },
        None,
    );
    let c = page.canvas();
    text(c, 54, 214, b"Opening your book", 32, true, 420);
    para(
        c,
        54,
        282,
        b"Getting your page ready. A book's first open takes a little longer.",
        24,
        420,
        398,
    );
    c.rect(54, 423, 420, 1, true);
    para(
        c,
        54,
        453,
        b"Press Back to cancel. Your saved position is kept.",
        21,
        420,
        544,
    );
}
pub fn render_reader_working_ui(work: SlowWork, settings: ViewSettings, frame: Option<&mut Frame>) {
    let Some(frame) = frame else { return };
    let line = match work {
        SlowWork::Saving => "Saving your place…",
        SlowWork::Applying => "Applying settings…",
        SlowWork::Loading => "Loading page…",
        _ => "Opening your book…",
    };
    let g = newt_core::preferences::geometry(settings);
    let (w, h) = (i32::from(g.width), i32::from(g.height));
    let mut c = UiCanvas::new(Some(frame), w, h);
    c.rect(0, 0, w, h, false);
    let (left, top) = ((w - 400) / 2, (h - 104) / 2);
    c.panel(left, top, 400, 104);
    c.clip(left, top, 400, 104);
    c.center(top + 36, Some(line.as_bytes()), 28, true);
}
