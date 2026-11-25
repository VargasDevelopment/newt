use crate::*;
use core::fmt::Write;
use newt_core::zip_container::Status;
pub use newt_device_ui::DeviceInfo;
#[derive(Clone, Copy)]
pub struct ReaderFooter<'a> {
    pub spine: u32,
    pub page: u32,
    pub pages: u32,
    pub overall_known: bool,
    pub overall_percent: u8,
    pub section_label: Option<&'a [u8]>,
}
#[derive(Clone, Copy)]
pub struct ReaderChapterFailure {
    pub spine: u32,
    pub status: Status,
    pub invalid_text: bool,
}
pub fn chapter_failure_message(f: &ReaderChapterFailure) -> &'static [u8] {
    if f.invalid_text {
        return b"This chapter can't be read (invalid text). Your place is kept.";
    }
    match f.status {
        Status::IoError | Status::Damaged => {
            b"This chapter can't be read (damaged data). Your place is kept."
        }
        Status::Truncated => b"This chapter can't be read (incomplete data). Your place is kept.",
        Status::ParseError => {
            b"This chapter can't be read (malformed content). Your place is kept."
        }
        Status::Ok
        | Status::NotZip
        | Status::NotEpub
        | Status::Encrypted
        | Status::Unsupported
        | Status::OutOfMemory
        | Status::NotFound
        | Status::Stale
        | Status::MetadataTooLarge
        | Status::PathTooLong => {
            b"This chapter can't be read (too large or unsupported). Your place is kept."
        }
    }
}
fn field(
    c: &mut UiCanvas<'_>,
    left: i32,
    right: i32,
    value: &mut [u8],
    centered: bool,
    footer_y: i32,
) {
    let mut p = 16;
    while p > 14 && width(value, p, false) > right - left {
        p -= 1;
    }
    newt_ui_canvas::ellipsize(value, p, right - left);
    let w = (right - left).min(width(value, p, false));
    text(
        c,
        if centered {
            left + (right - left - w) / 2
        } else {
            right - w
        },
        footer_y + 13 - p as i32,
        value,
        p,
        false,
        right - left,
    );
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FooterSlots {
    pub battery_max_w: i32,
    pub chapter_left: i32,
    pub chapter_right: i32,
    pub page_left: i32,
    pub page_right: i32,
    pub overall_left: i32,
    pub overall_right: i32,
}
/// Chapter-wide upper bound for any valid page counter in this chapter, derived
/// directly from the font atlas's digit advances and kerning (never a
/// hand-picked digit) and independent of the current page number.
fn chapter_page_max_width(page: &[u8]) -> i32 {
    use newt_ui_canvas::atlas;
    let bytes = cstr(page);
    let Some(slash) = bytes.iter().position(|&b| b == b'/') else {
        return width(page, 14, false);
    };
    let total = &bytes[slash + 1..];
    let digits = total.len().min(10) as i32;
    if digits == 0 {
        return width(page, 14, false);
    }
    let f = atlas::face(14, false);
    let mut max_first = 0i32;
    let mut max_step = 0i32;
    let mut max_slash_kern = atlas::kern(f, u32::from(b'0'), u32::from(b'/'));
    for a in u32::from(b'0')..=u32::from(b'9') {
        max_first = max_first.max(i32::from(atlas::glyph(f, a, 0).advance));
        max_slash_kern = max_slash_kern.max(atlas::kern(f, a, u32::from(b'/')));
        for b in u32::from(b'0')..=u32::from(b'9') {
            let step = i32::from(atlas::glyph(f, b, 0).advance) + atlas::kern(f, a, b);
            max_step = max_step.max(step);
        }
    }
    let num_w = max_first
        .saturating_add((digits - 1).saturating_mul(max_step))
        .saturating_add(max_slash_kern);
    let tail_w = atlas::width64(Some(&bytes[slash..]), f);
    atlas::pixels(num_w.saturating_add(tail_w), f, 14)
}
fn footer_slots_for(
    w: i32,
    m: i32,
    battery_text: &[u8],
    percent: &[u8],
    need_w: i32,
) -> FooterSlots {
    let radius = if w == newt_core::frame::WIDTH as i32 { 70 } else { 100 };
    let (mut cl, mut cr) = (w / 2 - radius, w / 2 + radius);
    let overall_right = w - m;
    let overall_left = overall_right - width(b"UNSAVED", 16, false);
    let mut page_right = overall_left - 20;
    let battery_left = m + 24;
    let default_slot = page_right - (cr + 12);
    if need_w > default_slot {
        let percent_w = width(b"100%", 16, false).max(width(percent, 16, false));
        page_right = (overall_right - percent_w - 12).max(page_right);
        let min_cl = battery_left + width(battery_text, 16, false) + 12;
        let min_cr = (min_cl + width(b"PG SAVE ERR", 14, false)).min(cr);
        let max_cr = page_right - 12 - need_w;
        if max_cr < cr {
            let new_cr = max_cr.max(min_cr);
            let shift = cr - new_cr;
            cr = new_cr;
            cl = (cl - shift).max(min_cl);
        }
    }
    FooterSlots {
        battery_max_w: cl - battery_left - 12,
        chapter_left: cl,
        chapter_right: cr,
        page_left: cr + 12,
        page_right,
        overall_left,
        overall_right,
    }
}
pub fn footer_slots(
    w: i32,
    m: i32,
    battery_text: &[u8],
    page: &[u8],
    pages: u32,
    percent: &[u8],
) -> FooterSlots {
    // Keep chapters whose worst-case counter fits the default slot untouched,
    // and expand consistently across the chapter once a 4+ digit counter overflows.
    let need_w = if pages != 0 {
        chapter_page_max_width(page)
    } else {
        width(page, 14, false)
    };
    footer_slots_for(w, m, battery_text, percent, need_w)
}
pub fn render_reader_chrome(
    f: &ReaderFooter<'_>,
    info: &DeviceInfo<'_>,
    settings: ViewSettings,
    ready: bool,
    frame: Option<&mut Frame>,
) {
    let Some(frame) = frame else { return };
    let g = newt_core::preferences::geometry(settings);
    if !ready {
        frame.fill(0);
    }
    let (w, h, m, bottom, fy) = (
        i32::from(g.width),
        i32::from(g.height),
        i32::from(g.margin),
        i32::from(g.content_bottom),
        i32::from(g.footer_y),
    );
    let mut c = UiCanvas::new(Some(frame), w, h);
    c.rect(0, bottom, w, h - bottom, false);
    c.clip(m, m, w - 2 * m, h - 2 * m);
    if !ready {
        let after = c.fixed_complete_paragraph(
            m,
            m + 24,
            Some(b"This book could not open. Your position is kept. Press Back to return Home."),
            24,
            w - 2 * m,
            bottom,
        );
        if let Some(storage) = info.storage() {
            if after >= 0 {
                para(&mut c, m, after + 16, storage, 21, w - 2 * m, bottom);
            }
        }
    }
    let battery = info.battery_percent.min(100);
    let overall = f.overall_percent.min(100);
    let mut battery_text = [0; 8];
    let mut chapter = [0; 96];
    let mut page = [0; 32];
    let mut percent = [0; 12];
    if info.battery_known {
        let _ = write!(Buffer::new(&mut battery_text), "{}%", battery);
    } else {
        let _ = write!(Buffer::new(&mut battery_text), "--");
    }
    if ready && f.pages != 0 && f.page < f.pages {
        newt_ui_canvas::chapter_label(f.section_label, f.spine, &mut chapter);
        let _ = write!(
            Buffer::new(&mut page),
            "{}/{}",
            u64::from(f.page) + 1,
            f.pages
        );
    } else {
        let _ = write!(Buffer::new(&mut chapter), "Section --");
        let _ = write!(Buffer::new(&mut page), "--/--");
    }
    if ready && (info.page_save_unconfirmed() || info.storage().is_some()) {
        let line = if info.page_save_unconfirmed() {
            "PG SAVE ERR"
        } else {
            "STORAGE"
        };
        let _ = write!(Buffer::new(&mut chapter), "{line}");
    }
    if ready && f.overall_known {
        let _ = write!(Buffer::new(&mut percent), "{}%", overall);
    } else {
        let _ = write!(Buffer::new(&mut percent), "--");
    }
    let active_pages = if ready && f.pages != 0 && f.page < f.pages {
        f.pages
    } else {
        0
    };
    let slots = footer_slots(w, m, &battery_text, &page, active_pages, &percent);
    let battery_left = m + 24;
    c.outline(m, fy + 2, 18, 10);
    c.rect(m + 18, fy + 5, 2, 4, true);
    if info.battery_known {
        c.rect(m + 2, fy + 4, i32::from(14 * battery / 100), 6, true);
    }
    text(
        &mut c,
        battery_left,
        fy - 3,
        &battery_text,
        16,
        false,
        slots.battery_max_w,
    );
    field(&mut c, slots.chapter_left, slots.chapter_right, &mut chapter, true, fy);
    field(&mut c, slots.page_left, slots.page_right, &mut page, false, fy);
    field(&mut c, slots.overall_left, slots.overall_right, &mut percent, false, fy);
    if settings.slim_progress_bar && ready && f.overall_known {
        c.outline(m, g.bar_y.into(), w - 2 * m, 3);
        c.rect(
            m,
            g.bar_y.into(),
            (w - 2 * m) * i32::from(overall) / 100,
            3,
            true,
        );
    }
}
pub fn render_chapter_failure(
    f: &ReaderChapterFailure,
    info: &DeviceInfo<'_>,
    settings: ViewSettings,
    frame: Option<&mut Frame>,
) {
    let Some(frame) = frame else { return };
    let g = newt_core::preferences::geometry(settings);
    let (w, h, m, bottom) = (
        i32::from(g.width),
        i32::from(g.height),
        i32::from(g.margin),
        i32::from(g.content_bottom),
    );
    let mut c = UiCanvas::new(Some(frame), w, h);
    c.rect(0, 0, w, bottom, false);
    c.clip(m, m, w - 2 * m, h - 2 * m);
    let mut after = c.fixed_complete_paragraph(
        m,
        m + 24,
        Some(chapter_failure_message(f)),
        24,
        w - 2 * m,
        bottom,
    );
    if after >= 0 {
        after = c.fixed_complete_paragraph(
            m,
            after + 16,
            Some(b"Press Next or Previous for another chapter, or Back for Home."),
            21,
            w - 2 * m,
            bottom,
        );
    }
    if let Some(storage) = info.storage() {
        if after >= 0 {
            para(&mut c, m, after + 16, storage, 21, w - 2 * m, bottom);
        }
    }
}
pub fn render_verification_notice(
    notice: ReaderUiNotice,
    settings: ViewSettings,
    ready: bool,
    frame: Option<&mut Frame>,
) {
    let Some(frame) = frame else { return };
    let g = newt_core::preferences::geometry(settings);
    let (w, h, m, bottom) = (
        i32::from(g.width),
        i32::from(g.height),
        i32::from(g.margin),
        i32::from(g.content_bottom),
    );
    let mut c = UiCanvas::new(Some(frame), w, h);
    let line: &[u8] = if notice == ReaderUiNotice::VerificationTimedOut {
        c.rect(m, bottom - 64, w - 2 * m, 64, false);
        b"Book check timed out. Press again."
    } else if notice == ReaderUiNotice::VerificationFailed && !ready {
        b"Could not check book. Press Back to reopen."
    } else {
        return;
    };
    c.fixed_complete_paragraph(m, bottom - 64, Some(line), 21, w - 2 * m, bottom);
}

#[cfg(test)]
mod footer_bound_tests {
    use super::*;

    fn chosen_size(label: &[u8], slot_w: i32) -> u32 {
        let mut p = 16;
        while p > 14 && width(label, p, false) > slot_w {
            p -= 1;
        }
        p
    }

    fn adjacent_2011_steady(slots_of: impl Fn(&[u8]) -> FooterSlots) -> bool {
        let labels: [&[u8]; 3] = [b"2003/2011", b"2004/2011", b"2005/2011"];
        let first = slots_of(labels[0]);
        let first_w = first.page_right - first.page_left;
        let first_p = chosen_size(labels[0], first_w);
        for label in labels {
            let s = slots_of(label);
            let sw = s.page_right - s.page_left;
            if s != first || chosen_size(label, sw) != first_p || width(label, first_p, false) > sw
            {
                return false;
            }
        }
        true
    }

    #[test]
    fn old_eight_bound_control_fails_adjacent_2011_steady_check() {
        let g = newt_core::preferences::geometry(ViewSettings {
            orientation: Orientation::Portrait,
            ..ViewSettings::default()
        });
        let (w, m) = (i32::from(g.width), i32::from(g.margin));
        // Real atlas-derived bound holds steady across 2003/2011 -> 2004/2011 -> 2005/2011.
        assert!(adjacent_2011_steady(|label| {
            footer_slots(w, m, b"100%", label, 2011, b"100%")
        }));
        // Negative control: the old '8'-filled bound underestimates '0'/'4' in the
        // pinned Inter atlas, so 2004/2011 jumps right and changes font size.
        assert!(!adjacent_2011_steady(|label| {
            let old_need = width(label, 14, false).max(width(b"8888/2011", 14, false));
            footer_slots_for(w, m, b"100%", b"100%", old_need)
        }));
    }
}
