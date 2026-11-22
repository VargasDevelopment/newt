use crate::*;
use core::fmt::Write;
use newt_core::{
    app::{StatsGraphPosition, StatsGraphView, stats_graph_position},
    progress::Progress,
    reading_stats::BookFacts,
    stats_graph_data::{StatsGraphs, stats_unknown_seconds},
};
use newt_device_ui::stats::{
    StatsAvailability, StatsBox, draw_stats_days, draw_stats_periods, format_stats_day,
    format_stats_duration, stats_finish_day, stats_graph_days,
};
// Share Device's caption policy with the reader statistics panel.
use newt_device_ui::stats::{days_caption, has_days, periods_caption};
#[derive(Clone, Copy)]
pub struct BookStatsView {
    pub ready: bool,
    pub history: StatsAvailability,
    pub seconds: u32,
    pub pages: u32,
    pub sessions: u32,
    pub imported_seconds: u32,
    pub dates_availability: StatsAvailability,
    pub facts: BookFacts,
    pub graphs: StatsGraphs,
}
impl Default for BookStatsView {
    fn default() -> Self {
        Self {
            ready: false,
            history: StatsAvailability::Ready,
            seconds: 0,
            pages: 0,
            sessions: 0,
            imported_seconds: 0,
            dates_availability: StatsAvailability::Unavailable,
            facts: BookFacts::default(),
            graphs: StatsGraphs::default(),
        }
    }
}
pub fn book_panel_days(settings: ViewSettings) -> u32 {
    stats_graph_days(overlay_geometry(settings).width - 44)
}
/// Card data plus the canonical live counters; never writes or folds a ledger.
#[allow(clippy::too_many_arguments)]
pub fn render_statistics_panel(
    ui: &ReaderUiState,
    settings: ViewSettings,
    title: Option<&[u8]>,
    progress: &Progress,
    ready: bool,
    frame: Option<&mut Frame>,
    book: Option<&BookStatsView>,
) {
    let mut view = book.copied().unwrap_or_default();
    if book.is_none() {
        view.history = StatsAvailability::Unavailable;
    }
    view.ready = ready && progress.book != 0;
    view.seconds = view
        .seconds
        .wrapping_add((progress.active_milliseconds / 1000) as u32);
    view.pages = view.pages.wrapping_add(progress.forward_pages);
    view.sessions = view.sessions.wrapping_add(progress.qualified_sessions);
    view.graphs.periods.unknown_seconds =
        stats_unknown_seconds(&view.graphs.periods.seconds, u64::from(view.seconds));
    render_book_stats_panel(
        frame,
        settings,
        title,
        &view,
        stats_graph_position(ui.selected as u8),
    );
}
fn whole(c: &mut UiCanvas<'_>, x: i32, y: i32, s: &[u8], p: u32, b: bool, left: i32, right: i32) {
    let w = width(s, p, b);
    if w > right - left {
        return;
    }
    text(c, x.clamp(left, right - w), y, s, p, b, w);
}
fn row(c: &mut UiCanvas<'_>, left: i32, right: i32, top: i32, label: &[u8], value: &[u8]) {
    text(c, left, top, label, 21, false, right - left);
    whole(
        c,
        right - width(value, 21, true),
        top,
        value,
        21,
        true,
        left + width(label, 21, false) + 12,
        right,
    );
}
fn summary(c: &mut UiCanvas<'_>, left: i32, right: i32, top: i32, v: &BookStatsView) {
    let mut value = [0; 32];
    format_stats_duration(v.seconds, &mut value);
    row(c, left, right, top, b"Time read", &value);
    let _ = write!(Buffer::new(&mut value), "{}", v.pages);
    row(c, left, right, top + 31, b"Pages turned", &value);
    let _ = write!(Buffer::new(&mut value), "{}", v.sessions);
    row(c, left, right, top + 62, b"Sessions", &value);
    if v.dates_availability != StatsAvailability::Ready {
        let line: &[u8] = match v.dates_availability {
            StatsAvailability::Counting => b"Dates: counting...",
            StatsAvailability::Newer => b"Dates: from a newer Newt",
            _ => b"Dates unavailable",
        };
        text(c, left, top + 93, line, 21, false, right - left);
        return;
    }
    let finish = stats_finish_day(&v.facts);
    for (i, (label, day)) in [
        (b"Started".as_slice(), v.facts.first_day),
        (b"Last read".as_slice(), v.facts.last_day),
        (b"Finished".as_slice(), finish),
    ]
    .into_iter()
    .enumerate()
    {
        if i == 2 && finish == newt_core::reading_stats::DAY_UNSET {
            let _ = write!(Buffer::new(&mut value), "Not yet");
        } else {
            format_stats_day(day, &mut value);
        }
        row(c, left, right, top + 93 + i as i32 * 31, label, &value);
    }
}
pub fn render_book_stats_panel(
    frame: Option<&mut Frame>,
    settings: ViewSettings,
    title: Option<&[u8]>,
    v: &BookStatsView,
    shown: StatsGraphPosition,
) {
    let Some(frame) = frame else { return };
    let g = newt_core::preferences::geometry(settings);
    let mut c = UiCanvas::new(Some(frame), g.width.into(), g.height.into());
    let OverlayGeometry {
        x,
        y,
        width: w,
        height: h,
    } = overlay_geometry(settings);
    c.clip(x, y, w, h);
    c.panel(x, y, w, h);
    text(
        &mut c,
        x + 22,
        y + 22,
        b"Reading statistics",
        28,
        true,
        w - 44,
    );
    let mut buf = [0; 128];
    if v.ready && title.is_some_and(|s| !cstr(s).is_empty()) {
        newt_ui_canvas::book_title(title, &mut buf);
    } else {
        let _ = write!(Buffer::new(&mut buf), "Book unavailable");
    }
    para(&mut c, x + 22, y + 59, &buf, 21, w - 44, y + 89);
    let (left, right, top, bottom) = (x + 22, x + w - 22, y + 100, y + h - 44);
    let name: &[u8] = match shown.view {
        StatsGraphView::Days => b"DAYS READ",
        StatsGraphView::Periods => b"TIME OF DAY",
        _ => b"THIS BOOK",
    };
    let name_right = left + width(name, 17, true);
    text(&mut c, left, top, name, 17, true, right - left);
    let box_ = StatsBox {
        x: left,
        y: top + 34,
        width: right - left,
        height: bottom - top - 34,
    };
    if !v.ready {
        text(
            &mut c,
            left,
            top + 36,
            b"Reading history unavailable",
            21,
            false,
            right - left,
        );
    } else if shown.view == StatsGraphView::Summary {
        summary(&mut c, left, right, top + 30, v);
        let mut note = [0; 80];
        let mut imported = [0; 24];
        format_stats_duration(v.imported_seconds, &mut imported);
        if v.imported_seconds != 0 {
            let _ = write!(
                Buffer::new(&mut note),
                "Includes {} read in CrossInk.",
                core::str::from_utf8(cstr(&imported)).unwrap_or("")
            );
        } else {
            let _ = write!(
                Buffer::new(&mut note),
                "Includes this session. Menus and sleep don't count."
            );
        }
        text(&mut c, left, bottom - 22, &note, 17, false, right - left);
    } else if v.history != StatsAvailability::Ready {
        let top = box_.y + (box_.height / 3 - 30).max(0);
        let (heading, note): (&[u8], &[u8]) = if v.history == StatsAvailability::Newer {
            (
                b"From a newer Newt",
                b"A newer version of Newt saved this book's days.",
            )
        } else {
            (
                b"Graphs unavailable",
                b"This book's days could not be read from the card.",
            )
        };
        whole(&mut c, left, top, heading, 21, true, left, right);
        para(
            &mut c,
            left,
            top + 34,
            note,
            17,
            right - left,
            box_.y + box_.height,
        );
    } else {
        buf.fill(0);
        if shown.view == StatsGraphView::Days && has_days(&v.graphs.days) {
            days_caption(&v.graphs.days, &mut buf);
        } else if shown.view == StatsGraphView::Periods {
            periods_caption(&v.graphs.periods, &mut buf);
        }
        if buf[0] != 0 {
            whole(
                &mut c,
                right - width(&buf, 17, false),
                top,
                &buf,
                17,
                false,
                name_right + 16,
                right,
            );
        }
        if shown.view == StatsGraphView::Days {
            draw_stats_days(&mut c, box_, &v.graphs.days);
        } else {
            draw_stats_periods(&mut c, box_, &v.graphs.periods);
        }
    }
    let hint: &[u8] = if !v.ready || shown.view == StatsGraphView::Summary {
        "Next: days read · Back: menu".as_bytes()
    } else if shown.view == StatsGraphView::Days {
        "Prev/Next: views · Select: earlier · Back: menu".as_bytes()
    } else {
        "Prev: days read · Back: menu".as_bytes()
    };
    text(&mut c, left, y + h - 32, hint, 17, false, right - left);
}
