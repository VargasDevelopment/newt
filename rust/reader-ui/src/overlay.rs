use crate::*;
use core::fmt::Write;
use newt_core::progress::Progress;
use newt_phased_reader::ChapterMenuView;
/// The menu view borrows canonical navigation for this paint only.
#[allow(clippy::too_many_arguments)]
pub fn render_reader_overlay(
    ui: &ReaderUiState,
    settings: ViewSettings,
    title: Option<&[u8]>,
    progress: &Progress,
    ready: bool,
    frame: Option<&mut Frame>,
    chapters: Option<ChapterMenuView<'_>>,
    failure_line: Option<&[u8]>,
    book: Option<&BookStatsView>,
) {
    let Some(frame) = frame else { return };
    if ui.panel == ReaderPanel::Page {
        return;
    }
    if ui.panel == ReaderPanel::Statistics {
        return render_statistics_panel(ui, settings, title, progress, ready, Some(frame), book);
    }
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
    let heading: &[u8] = match ui.panel {
        ReaderPanel::Chapters => b"Chapters",
        ReaderPanel::Settings => b"Reading settings",
        _ => b"Reading menu",
    };
    text(&mut c, x + 22, y + 22, heading, 28, true, w - 44);
    let mut shown = [0; 128];
    if ready && title.is_some_and(|s| !cstr(s).is_empty()) {
        newt_ui_canvas::book_title(title, &mut shown);
    } else {
        let _ = write!(Buffer::new(&mut shown), "Book unavailable");
    }
    para(&mut c, x + 22, y + 59, &shown, 21, w - 44, y + 89);
    let row_y = y + 100;
    match ui.panel {
        ReaderPanel::Menu => {
            if ready {
                let labels: [&[u8]; 6] = [
                    b"Return to reading",
                    b"Chapters",
                    b"Reading settings",
                    b"Reading statistics",
                    if ui.finish == ReaderFinishRow::Undo {
                        b"Mark as not finished"
                    } else {
                        b"Mark as finished"
                    },
                    b"Home",
                ];
                let hidden = ui.finish == ReaderFinishRow::Hidden;
                let focus = if ui.selected < 6 && !(hidden && ui.selected == 4) {
                    ui.selected
                } else {
                    5
                };
                let mut shown = 0;
                for (id, label) in labels.into_iter().enumerate() {
                    if id == 4 && hidden {
                        continue;
                    }
                    c.focus(
                        x + 18,
                        row_y + shown * 43,
                        w - 40,
                        Some(label),
                        id as u32 == focus,
                    );
                    shown += 1;
                }
            } else {
                c.focus(
                    x + 18,
                    row_y,
                    w - 40,
                    Some(b"Reading settings"),
                    ui.selected != 5,
                );
                c.focus(x + 18, row_y + 43, w - 40, Some(b"Home"), ui.selected == 5);
            }
        }
        ReaderPanel::Chapters => {
            let info = chapters.as_ref().map(|v| v.info()).unwrap_or_default();
            let fallback = chapters.as_ref().map(|v| v.fallback()).unwrap_or_default();
            if !ready || !info.valid || info.count == 0 {
                text(
                    &mut c,
                    x + 22,
                    row_y + 12,
                    b"No chapters available",
                    21,
                    false,
                    w - 44,
                );
            } else {
                if let Some(chapters) = chapters {
                    for i in 0..u32::from(info.visible) {
                        let chapter = chapters.row(info.first + i);
                        if !chapter.valid {
                            continue;
                        }
                        let mut label = [0; 96];
                        newt_ui_canvas::chapter_label(
                            Some(chapter.label),
                            chapter.spine,
                            &mut label,
                        );
                        let top = row_y + i as i32 * 43;
                        let marker = if chapter.current {
                            width(b"/ current", 17, true) + 18
                        } else {
                            0
                        };
                        if chapter.current {
                            c.clip(x, y, w - 30 - marker, h);
                        }
                        c.focus(
                            x + 18,
                            top,
                            w - 40,
                            Some(&label),
                            chapter.index == info.focus,
                        );
                        if chapter.current {
                            c.clip(x, y, w, h);
                            text(
                                &mut c,
                                x + w - 30 - marker,
                                top + 10,
                                b"/ current",
                                17,
                                true,
                                marker,
                            );
                        }
                    }
                }
                let line: &[u8] =
                    if ui.notice == ReaderUiNotice::ChapterJumpFailed && failure_line.is_some() {
                        failure_line.unwrap_or(b"")
                    } else if ui.notice == ReaderUiNotice::ChapterLocationUnresolved {
                        b"That chapter's location could not be found"
                    } else if fallback == newt_phased_reader::ContentsFallback::Capacity {
                        b"Contents too large; numbers shown."
                    } else if fallback == newt_phased_reader::ContentsFallback::Invalid {
                        b"Contents unreadable; numbers shown."
                    } else if info.authored {
                        b"From the book's contents"
                    } else {
                        b"This book has no contents list"
                    };
                text(&mut c, x + 22, y + h - 63, line, 21, false, w - 44);
            }
        }
        ReaderPanel::Settings => {
            let mut orientation = [0; 64];
            let mut refresh = [0; 48];
            let _ = write!(
                Buffer::new(&mut orientation),
                "Orientation: {}",
                if ui.draft.orientation == Orientation::LandscapeCcw {
                    "Landscape CCW"
                } else {
                    "Portrait"
                }
            );
            let _ = write!(
                Buffer::new(&mut refresh),
                "Full refresh: every {} page{}",
                ui.draft.refresh_pages,
                if ui.draft.refresh_pages == 1 { "" } else { "s" }
            );
            let labels: [&[u8]; 5] = [
                &orientation,
                if ui.draft.slim_progress_bar {
                    b"Slim progress bar: On"
                } else {
                    b"Slim progress bar: Off"
                },
                &refresh,
                if ui.preferences_writable {
                    b"Save and return"
                } else {
                    b"Save unavailable"
                },
                b"Cancel",
            ];
            for (i, label) in labels.into_iter().enumerate() {
                c.focus(
                    x + 18,
                    row_y + i as i32 * 43,
                    w - 40,
                    Some(label),
                    i as u32 == ui.selected.min(4),
                );
            }
            if !ui.preferences_writable {
                text(
                    &mut c,
                    x + 22,
                    y + h - 91,
                    b"Settings storage unavailable",
                    21,
                    false,
                    w - 44,
                );
            } else if ui.notice == ReaderUiNotice::SaveFailed {
                text(
                    &mut c,
                    x + 22,
                    y + h - 91,
                    b"Could not verify save. Draft kept.",
                    21,
                    false,
                    w - 44,
                );
            }
            text(
                &mut c,
                x + 22,
                y + h - 63,
                b"Book only. Home stays portrait.",
                21,
                false,
                w - 44,
            );
        }
        _ => {}
    }
    let hint: &[u8] = match (ui.panel, ui.notice) {
        (ReaderPanel::Menu, ReaderUiNotice::FinishSaveFailed) => b"Could not save. Try again.",
        (ReaderPanel::Menu, ReaderUiNotice::FinishUnconfirmed) => {
            b"Save unconfirmed. Restart may use old or new."
        }
        (ReaderPanel::Menu, ReaderUiNotice::FinishOpening) => {
            b"Still opening this book. Try again soon."
        }
        _ => b"Prev/Next: choose / Select / Back",
    };
    text(&mut c, x + 22, y + h - 32, hint, 17, false, w - 44);
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use newt_phased_reader::{ContentsFallback, PhasedReader, Selection};
    use newt_reader_navigation::{Section, metadata::Metadata};
    use newt_core::{package_parsers::{ManifestItem, PackageResult, TocEntry}, zip_window::arena::Arena};
    use std::boxed::Box;

    #[test]
    fn contents_overlay_explains_capacity_damage_and_absence() {
        let settings = ViewSettings { orientation: Orientation::Portrait, ..Default::default() };
        let ui = ReaderUiState { panel: ReaderPanel::Chapters, ..Default::default() };
        let OverlayGeometry { x, y, width: w, height: h } = overlay_geometry(settings);
        assert_eq!(newt_core::preferences::geometry(settings).width, 528);
        for (fallback, authored, expected) in [
            (ContentsFallback::Capacity, false, b"Contents too large; numbers shown.".as_slice()),
            (ContentsFallback::Invalid, false, b"Contents unreadable; numbers shown.".as_slice()),
            (ContentsFallback::None, false, b"This book has no contents list".as_slice()),
            (ContentsFallback::None, true, b"From the book's contents".as_slice()),
        ] {
            assert!(width(expected, 21, false) <= w - 44, "portrait notice is clipped");
            let mut bytes = [0; 64];
            let mut arena = Arena::new(&mut bytes);
            let href = arena.copy_nul_ref(Some(b"chapter.xhtml"));
            let title = arena.copy_nul_ref(Some(b"Start"));
            let media_type = arena.copy_nul_ref(Some(b"application/xhtml+xml"));
            let manifest = [ManifestItem { href, media_type, ..Default::default() }];
            let entries = [newt_core::zip_container::Entry { name: href,
                name_hash: newt_core::zip_container::hash_path(b"chapter.xhtml"), ..Default::default() }];
            let toc = [TocEntry { href, title, ..Default::default() }];
            let package = PackageResult { spine_count: 1, ..Default::default() };
            let book = Metadata { arena: &arena, package: &package,
                manifest: &manifest, spine: &[0], toc: if authored { &toc } else { &[] }, entries: &entries };
            let mut selections = [Selection::default(), Selection::default()];
            assert!(selections[1].navigation.bind_menu(&book, Section::default(), 0, &mut [0]));
            selections[1].navigation.set_contents_fallback(fallback);
            let reader = PhasedReader::new(&mut selections, None, false);
            assert!(reader.chapter_menu().info().valid);
            assert_eq!(reader.chapter_menu().info().authored, authored);
            let mut actual = Box::new([0; newt_core::frame::FRAME_BYTES]);
            render_reader_overlay(&ui, settings, None, &Progress::default(), true,
                Some(&mut actual), Some(reader.chapter_menu()), None, None);
            // Replace only the notice with its independently specified text;
            // the rest of the real overlay must remain identical.
            let mut wanted = actual.clone();
            let g = newt_core::preferences::geometry(settings);
            let mut canvas = UiCanvas::new(Some(&mut wanted), g.width.into(), g.height.into());
            canvas.rect(x + 22, y + h - 63, w - 44, 26, false);
            text(&mut canvas, x + 22, y + h - 63, expected, 21, false, w - 44);
            assert!(actual[..] == wanted[..], "wrong Contents footer for {fallback:?}");
        }
    }
}
