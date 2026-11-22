//! Painting only. Graph aggregation, calendar, buckets and summary belong to Core.
use crate::screens::MONTHS;
use crate::{Buffer, SystemButtons, UiCanvas, cstr, para, text, width};
use core::fmt::Write;
use newt_core::{
    app::StatsGraphView,
    reading_stats::*,
    stats_clock::{stats_bucket, stats_date},
    stats_graph_data::*,
};
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum StatsAvailability {
    Ready,
    Counting,
    #[default]
    Unavailable,
    Newer,
}
#[derive(Default)]
pub struct StatsPageView {
    pub availability: StatsAvailability,
    pub summary: StatsSummary,
}
#[derive(Default)]
pub struct StatsScreenView {
    pub page: StatsPageView,
    pub graphs: StatsGraphs,
}
#[derive(Clone, Copy)]
pub struct StatsBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
pub fn format_stats_duration(seconds: u32, out: &mut [u8]) {
    let m = seconds / 60;
    let mut b = Buffer::new(out);
    let _ = if m < 60 {
        write!(b, "{m} min")
    } else {
        write!(b, "{} h {:02} min", m / 60, m % 60)
    };
}
pub fn format_stats_day(day: u16, out: &mut [u8]) {
    let mut b = Buffer::new(out);
    let _ = match day {
        DAY_UNSET => b.write_str("Not recorded"),
        DAY_UNDATED => b.write_str("No date"),
        DAY_UNKNOWN => b.write_str("Unknown"),
        _ => {
            let d = stats_date(day);
            write!(
                b,
                "{} {} {}",
                d.day,
                MONTHS[(usize::from(d.month) + 11) % 12],
                d.year
            )
        }
    };
}
pub fn stats_finish_day(f: &BookFacts) -> u16 {
    if !finished(f) {
        DAY_UNSET
    } else if f.end_day != DAY_UNSET {
        f.end_day
    } else {
        f.manual_finish_day
    }
}
fn summary_row(c: &mut UiCanvas<'_>, top: i32, label: &[u8], value: &[u8]) {
    text(c, 50, top, label, 21, false, 260);
    text(
        c,
        478 - width(value, 24, true),
        top - 2,
        value,
        24,
        true,
        168,
    );
}
fn count(c: &mut UiCanvas<'_>, top: i32, label: &[u8], value: u32, one: &str, many: &str) {
    let mut buf = [0; 32];
    let _ = write!(
        Buffer::new(&mut buf),
        "{} {}",
        value,
        if value == 1 { one } else { many }
    );
    summary_row(c, top, label, &buf);
}
pub fn render_stats_page(c: &mut UiCanvas<'_>, view: &StatsPageView) {
    text(c, 50, 139, b"ALL BOOKS ON THIS CARD", 17, true, 428);
    let unavailable = match view.availability {
        StatsAvailability::Ready => None,
        StatsAvailability::Counting => Some((
            "Counting...",
            "Newt is adding up the reading saved for every book on this card. Totals appear here when it is done.",
        )),
        StatsAvailability::Unavailable => Some((
            "Statistics unavailable",
            "The card could not be read. Check that it is inserted, then open this page again.",
        )),
        StatsAvailability::Newer => Some((
            "From a newer Newt",
            "A newer version of Newt saved these statistics. This version leaves them unchanged.",
        )),
    };
    if let Some((h, n)) = unavailable {
        text(c, 50, 190, h.as_bytes(), 24, true, 428);
        para(c, 50, 236, n.as_bytes(), 21, 428, 420);
        return;
    }
    let s = &view.summary;
    let mut buf = [0; 32];
    format_stats_duration(s.total_seconds, &mut buf);
    summary_row(c, 190, b"Time read", &buf);
    let mut top = 238;
    if s.dated {
        format_stats_duration(s.today_seconds, &mut buf);
        summary_row(c, top, b"Today", &buf);
        format_stats_duration(s.week_seconds, &mut buf);
        summary_row(c, top + 48, b"Last 7 days", &buf);
        count(
            c,
            top + 96,
            b"Current streak",
            s.current_streak.into(),
            "day",
            "days",
        );
        count(
            c,
            top + 144,
            b"Longest streak",
            s.longest_streak.into(),
            "day",
            "days",
        );
        top += 192;
    }
    count(
        c,
        top,
        b"Books finished",
        s.books_finished.into(),
        "book",
        "books",
    );
    count(c, top + 48, b"Sessions", s.sessions, "session", "sessions");
    count(c, top + 96, b"Pages turned", s.pages, "page", "pages");
    top += 160;
    if s.cross_ink {
        text(
            c,
            50,
            top,
            b"Includes your CrossInk history.",
            21,
            false,
            428,
        );
        top += 48;
    }
    if !s.dated {
        para(
            c,
            50,
            top,
            b"Dates need a set clock. Newt sets the clock when it checks for updates in Settings, Update.",
            21,
            428,
            top + 100,
        );
    }
}
pub fn stats_graph_days(w: i32) -> u32 {
    (w / 13 / 7).clamp(1, 6) as u32 * 7
}
pub fn stats_day_axis(seconds: u32, most: u32) -> (u32, u32) {
    let minutes = seconds.wrapping_add(59) / 60;
    for top in [
        5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 300, 360, 480, 600, 720, 900, 1080, 1200,
        1440,
    ] {
        if top < minutes {
            continue;
        }
        for step in [1, 2, 5, 10, 15, 30, 60, 120, 180, 240, 360, 480] {
            if top % step == 0 && (3..=most).contains(&(top / step)) {
                return (step, top / step);
            }
        }
    }
    ((minutes + 179) / 180 * 60, 3)
}
pub fn stats_graph_slot(g: &StatsDaysGraph, day: u16) -> i32 {
    if g.last_day == DAY_UNSET || day > g.last_day || g.last_day - day >= u16::from(g.count) {
        -1
    } else {
        i32::from(g.count) - 1 - i32::from(g.last_day - day)
    }
}
fn slot_day(g: &StatsDaysGraph, i: usize) -> u16 {
    g.last_day
        .wrapping_sub(u16::from(g.count).wrapping_sub(1).wrapping_sub(i as u16))
}
fn short_date(day: u16, out: &mut [u8]) {
    let d = stats_date(day);
    let _ = write!(
        Buffer::new(out),
        "{} {}",
        d.day,
        MONTHS[(usize::from(d.month) + 11) % 12]
    );
}
fn whole(
    c: &mut UiCanvas<'_>,
    x: i32,
    y: i32,
    s: &[u8],
    p: u32,
    b: bool,
    left: i32,
    right: i32,
) -> i32 {
    let w = width(s, p, b);
    if w > right - left {
        return -1;
    }
    let x = x.clamp(left, right - w);
    text(c, x, y, s, p, b, w);
    x + w
}
fn dotted(c: &mut UiCanvas<'_>, x: i32, y: i32, w: i32, step: usize) {
    for i in (0..w).step_by(step) {
        c.pixel(x + i, y, true);
    }
}
fn empty(c: &mut UiCanvas<'_>, b: StatsBox, h: &[u8], n: &[u8]) {
    let top = b.y + (b.height / 3 - 30).max(0);
    whole(c, b.x, top, h, 21, true, b.x, b.x + b.width);
    para(c, b.x, top + 34, n, 17, b.width, b.y + b.height);
}
fn axis_label(m: u32, step: u32, out: &mut [u8]) {
    let mut b = Buffer::new(out);
    let _ = if step < 30 || m < 60 {
        write!(b, "{m} min")
    } else if m % 60 != 0 {
        write!(b, "{}.5 h", m / 60)
    } else {
        write!(b, "{} h", m / 60)
    };
}
/// Invalid counts are rejected; valid Core windows have 0..=42 slots.
pub fn draw_stats_days(c: &mut UiCanvas<'_>, b: StatsBox, g: &StatsDaysGraph) {
    if usize::from(g.count) > STATS_GRAPH_MAX_DAYS {
        return;
    }
    if g.last_day == DAY_UNSET || g.count == 0 {
        return empty(c,b,b"Dates need a set clock",b"The days you read appear here once the clock is set. Newt sets the clock when it checks for updates.");
    }
    let mut recorded = false;
    let mut read = false;
    let mut unrecorded = false;
    let mut untimed = false;
    let mut estimated = false;
    let mut longest = 0;
    for i in 0..usize::from(g.count) {
        let cross = g.kinds[i] == StatsDayKind::Untimed;
        recorded |= g.kinds[i] != StatsDayKind::Unrecorded;
        read |= g.seconds[i] != 0 || cross;
        unrecorded |= g.kinds[i] == StatsDayKind::Unrecorded;
        untimed |= cross && g.seconds[i] == 0;
        estimated |= cross && g.seconds[i] != 0;
        longest = longest.max(g.seconds[i]);
    }
    if !read {
        return empty(
            c,
            b,
            if recorded {
                b"No reading on these days"
            } else {
                b"No days recorded yet"
            },
            b"Each day you read shows here as a bar, its height the time read.",
        );
    }
    let right = b.x + b.width;
    let legend = i32::from(unrecorded) + i32::from(untimed) + i32::from(estimated);
    let top = b.y + 10;
    let baseline = b.y + b.height - legend * 24 - 28;
    let h = baseline - top;
    if h < 12 {
        return;
    }
    let (step, lines) = stats_day_axis(longest, (h / 22).clamp(3, 6) as u32);
    let maximum = step * lines;
    let mut buf = [0; 24];
    let mut column = width(b"0", 15, false);
    for line in 1..=lines {
        axis_label(step * line, step, &mut buf);
        column = column.max(width(&buf, 15, false));
    }
    column += 8;
    let slot = (b.width - column) / i32::from(g.count);
    let left = right - slot * i32::from(g.count);
    if slot < 4 {
        return;
    }
    whole(
        c,
        left - 8 - width(b"0", 15, false),
        baseline - 12,
        b"0",
        15,
        false,
        b.x,
        left,
    );
    for line in 1..=lines {
        let y = baseline - (line * h as u32 / lines) as i32;
        axis_label(step * line, step, &mut buf);
        whole(
            c,
            left - 8 - width(&buf, 15, false),
            y - 10,
            &buf,
            15,
            false,
            b.x,
            left,
        );
        dotted(c, left, y, right - left, 4);
    }
    let bar = slot - 3.max(slot / 4);
    let mut label_right = b.x - 1;
    for i in 0..usize::from(g.count) {
        let x = left + i as i32 * slot;
        let bx = x + (slot - bar) / 2;
        if g.kinds[i] == StatsDayKind::Unrecorded {
            dotted(c, x, baseline, slot, 2);
        } else {
            c.rect(x, baseline, slot, 2, true);
        }
        let scaled = (u64::from(g.seconds[i]) * h as u64 / u64::from(maximum * 60)) as i32;
        if g.kinds[i] == StatsDayKind::Untimed {
            let bh = if g.seconds[i] != 0 {
                scaled.clamp(12, h)
            } else {
                8
            };
            c.outline(bx, baseline - bh, bar, bh);
        } else if g.seconds[i] != 0 {
            let bh = scaled.clamp(3, h);
            c.rect(bx, baseline - bh, bar, bh, true);
        }
        let day = slot_day(g, i);
        if stats_weekday(day) != 0 {
            continue;
        }
        let d = stats_date(day);
        if d.day <= 7 || label_right < b.x {
            short_date(day, &mut buf);
        } else {
            let _ = write!(Buffer::new(&mut buf), "{}", d.day);
        }
        let centre = x + slot / 2;
        let w = width(&buf, 15, false);
        if centre - w / 2 < label_right + 8 {
            continue;
        }
        c.rect(centre, baseline + 2, 1, 4, true);
        let end = whole(c, centre - w / 2, baseline + 8, &buf, 15, false, b.x, right);
        if end >= 0 {
            label_right = end;
        }
    }
    let mut y = baseline + 30;
    if unrecorded {
        dotted(c, b.x, y + 9, 24, 2);
        whole(c, b.x + 32, y, b"Not recorded", 15, false, b.x, right);
        y += 24;
    }
    if estimated {
        c.outline(b.x + 7, y + 1, 10, 14);
        whole(
            c,
            b.x + 32,
            y,
            b"Estimated from CrossInk",
            15,
            false,
            b.x,
            right,
        );
        y += 24;
    }
    if untimed {
        c.outline(b.x + 7, y + 4, 10, 8);
        whole(
            c,
            b.x + 32,
            y,
            b"Read in CrossInk, time not kept",
            15,
            false,
            b.x,
            right,
        );
    }
}
pub fn stats_period_name(i: usize) -> &'static [u8] {
    match i {
        0 => b"Morning",
        1 => b"Afternoon",
        2 => b"Evening",
        3 => b"Night",
        _ => b"Unknown",
    }
}
pub fn format_stats_period_hours(period: usize, out: &mut [u8]) {
    let mut start = None;
    let mut end = None;
    for h in 0..24u8 {
        let at = stats_bucket(h) as usize;
        let before = stats_bucket((h + 23) % 24) as usize;
        if at == period && before != period {
            start = Some(h);
        }
        if at != period && before == period {
            end = Some(h);
        }
    }
    let mut b = Buffer::new(out);
    let _ = if let (Some(s), Some(e)) = (start, end) {
        write!(b, "{s}:00-{e}:00")
    } else {
        b.write_str("No clock or earlier")
    };
}
pub fn draw_stats_periods(c: &mut UiCanvas<'_>, b: StatsBox, p: &StatsPeriodsGraph) {
    let rows = if p.unknown_seconds != 0 { 5 } else { 4 };
    let value = |i: usize| {
        if i < 4 {
            p.seconds[i]
        } else {
            p.unknown_seconds
        }
    };
    let largest = (0..rows).map(value).max().unwrap_or(0);
    if largest == 0 {
        return empty(
            c,
            b,
            b"No reading yet",
            b"The time of day you read appears after your first reading.",
        );
    }
    let right = b.x + b.width;
    let pitch = 52.min(b.height / rows as i32);
    let hours = pitch >= 42;
    let mut buf = [0; 24];
    let mut column = 0;
    let mut values = 0;
    for i in 0..rows {
        column = column.max(width(stats_period_name(i), 19, false));
        format_stats_period_hours(i, &mut buf);
        if hours {
            column = column.max(width(&buf, 15, false));
        }
        format_stats_duration(value(i), &mut buf);
        values = values.max(width(&buf, 19, true));
    }
    let left = (b.x + column + 12).min(right);
    let bar_right = (right - values - 12).max(left);
    let bw = bar_right - left;
    if bw != 0 {
        c.rect(left - 1, b.y, 1, pitch * rows as i32, true);
    }
    for i in 0..rows {
        let middle = b.y + i as i32 * pitch + pitch / 2;
        let seconds = value(i);
        let nt = if hours { middle - 21 } else { middle - 12 };
        whole(c, b.x, nt, stats_period_name(i), 19, false, b.x, left);
        if hours {
            format_stats_period_hours(i, &mut buf);
            whole(c, b.x, nt + 23, &buf, 15, false, b.x, left);
        }
        if seconds != 0 && bw >= 4 {
            let w = 2.max((u64::from(seconds) * bw as u64 / u64::from(largest)) as i32);
            if i < 4 {
                c.rect(left, middle - 7, w, 14, true);
            } else {
                c.outline(left, middle - 7, w.max(4), 14);
            }
        }
        format_stats_duration(seconds, &mut buf);
        whole(
            c,
            right - width(&buf, 19, true),
            middle - 12,
            &buf,
            19,
            true,
            bar_right,
            right,
        );
    }
}
pub fn has_days(g: &StatsDaysGraph) -> bool {
    g.kinds[..usize::from(g.count).min(STATS_GRAPH_MAX_DAYS)]
        .iter()
        .any(|k| *k != StatsDayKind::Unrecorded)
}
fn window(g: &StatsDaysGraph) -> (u32, u32, usize) {
    let mut total = 0u32;
    let mut read = 0;
    let mut longest = usize::from(g.count);
    let mut most = 0;
    for i in 0..usize::from(g.count).min(STATS_GRAPH_MAX_DAYS) {
        let cross = g.kinds[i] == StatsDayKind::Untimed;
        if cross || g.seconds[i] >= u32::from(STREAK_SECONDS) {
            read += 1;
        }
        if cross {
            continue;
        }
        total = total.wrapping_add(g.seconds[i]);
        if g.seconds[i] > most {
            most = g.seconds[i];
            longest = i;
        }
    }
    (total, read, longest)
}
pub fn days_caption(g: &StatsDaysGraph, out: &mut [u8]) {
    let mut from = [0; 12];
    let mut to = [0; 12];
    let mut time = [0; 24];
    short_date(slot_day(g, 0), &mut from);
    short_date(g.last_day, &mut to);
    format_stats_duration(window(g).0, &mut time);
    let _ = write!(
        Buffer::new(out),
        "{} - {} · {}",
        ascii(&from),
        ascii(&to),
        ascii(&time)
    );
}
fn ascii(s: &[u8]) -> &str {
    core::str::from_utf8(cstr(s)).unwrap_or("")
}
pub fn periods_caption(p: &StatsPeriodsGraph, out: &mut [u8]) {
    let total = p
        .seconds
        .iter()
        .fold(p.unknown_seconds, |a, b| a.wrapping_add(*b));
    out[0] = 0;
    if total == 0 {
        return;
    }
    let mut time = [0; 24];
    format_stats_duration(total, &mut time);
    let _ = write!(Buffer::new(out), "All time · {}", ascii(&time));
}
fn section(c: &mut UiCanvas<'_>, name: &[u8], caption: &[u8], height: i32) -> StatsBox {
    text(c, 50, 172, name, 24, true, 428);
    if !cstr(caption).is_empty() {
        text(c, 50, 206, caption, 19, false, 428);
    }
    StatsBox {
        x: 50,
        y: 242,
        width: 428,
        height,
    }
}
pub fn stats_graph_buttons(v: StatsGraphView) -> SystemButtons<'static> {
    SystemButtons {
        back: Some(b"Back"),
        confirm: if v == StatsGraphView::Days {
            Some(b"Earlier")
        } else {
            None
        },
        previous: if v == StatsGraphView::Summary {
            None
        } else {
            Some(b"Prev")
        },
        next: if v == StatsGraphView::Periods {
            None
        } else {
            Some(b"Next")
        },
    }
}
pub fn render_stats_graph_view(c: &mut UiCanvas<'_>, v: StatsGraphView, data: &StatsScreenView) {
    if v == StatsGraphView::Summary || data.page.availability != StatsAvailability::Ready {
        return render_stats_page(c, &data.page);
    }
    if v == StatsGraphView::Days && usize::from(data.graphs.days.count) > STATS_GRAPH_MAX_DAYS {
        return;
    }
    text(c, 50, 139, b"ALL BOOKS ON THIS CARD", 17, true, 428);
    let mut caption = [0; 64];
    if v == StatsGraphView::Periods {
        let p = &data.graphs.periods;
        periods_caption(p, &mut caption);
        let b = section(
            c,
            b"Time of day",
            &caption,
            if p.unknown_seconds != 0 { 260 } else { 208 },
        );
        draw_stats_periods(c, b, p);
        if p.unknown_seconds != 0 {
            para(c,50,b.y+b.height+20,b"Unknown is time read before Newt kept the time of day, or while the clock was not set.",19,428,700);
        }
        return;
    }
    let g = &data.graphs.days;
    if has_days(g) {
        days_caption(g, &mut caption);
    }
    let b = section(c, b"Days read", &caption, 300);
    draw_stats_days(c, b, g);
    if !has_days(g) {
        return;
    }
    let (_, read, longest) = window(g);
    let _ = write!(
        Buffer::new(&mut caption),
        "Read on {} of {} days",
        read,
        g.count
    );
    text(c, 50, b.y + b.height + 20, &caption, 21, false, 428);
    if longest < usize::from(g.count) {
        let mut time = [0; 24];
        let mut day = [0; 12];
        format_stats_duration(g.seconds[longest], &mut time);
        short_date(slot_day(g, longest), &mut day);
        let _ = write!(
            Buffer::new(&mut caption),
            "Longest day: {}, {}",
            ascii(&time),
            ascii(&day)
        );
        text(c, 50, b.y + b.height + 54, &caption, 21, false, 428);
    }
}
