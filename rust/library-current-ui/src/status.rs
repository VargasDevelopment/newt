// SPDX-License-Identifier: MIT
//! Existing loading, absent-card and unavailable shelf compositions.
use crate::*;
pub(crate) fn render_status(catalog: Option<&Catalog>, state: &LibraryUiState<'_>,
    frame: &mut Frame, bar: &SystemBar,
) -> bool {
    let count = catalog.map_or(0, Catalog::size);
    if !state.loading && !state.no_card && catalog.is_some_and(|c| c.status() == LibraryStatus::Ok) && count != 0 {
        return false;
    }
    drop(SystemPage::new(Some(frame), bar, buttons(catalog, state), Some(b"Books")));
    if state.loading {
        let c = &mut shelf_canvas(frame);
        c.text(50, 170, Some(b"Loading your library"), 28, true, 428, true);
        c.paragraph(
            50,
            224,
            Some(b"Looking for books on your card."),
            24,
            428,
            370,
        );
    } else if state.no_card {
        let c = &mut shelf_canvas(frame);
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
        let c = &mut shelf_canvas(frame);
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
    }
    if let Some(note) = note(catalog, state) {
        shelf_canvas(frame).text(50, 679, Some(note), 18, false, 428, true);
    }
    true
}
