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
    let radius = if w == newt_core::frame::WIDTH as i32 { 70 } else { 100 };
    let (cl, cr) = (w / 2 - radius, w / 2 + radius);
    let overall_left = w - m - width(b"UNSAVED", 16, false);
    let page_right = overall_left - 20;
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
        cl - battery_left - 12,
    );
    field(&mut c, cl, cr, &mut chapter, true, fy);
    field(&mut c, cr + 12, page_right, &mut page, false, fy);
    field(&mut c, overall_left, w - m, &mut percent, false, fy);
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
