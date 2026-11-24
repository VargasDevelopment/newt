use newt_reader_ui::*;
#[test]
fn dispatch_boundaries_and_storage_intents() {
    let settings = ViewSettings::default();
    // Expected results are literal pinned behavior, not computed by another dispatcher.
    let cases = [
        (
            ReaderPanel::Page,
            0,
            Action::Activate,
            ReaderPanel::Menu,
            0,
            ReaderUiIntent::Repaint,
        ),
        (
            ReaderPanel::Page,
            0,
            Action::Back,
            ReaderPanel::Page,
            0,
            ReaderUiIntent::Home,
        ),
        (
            ReaderPanel::Page,
            0,
            Action::Sleep,
            ReaderPanel::Page,
            0,
            ReaderUiIntent::Sleep,
        ),
        (
            ReaderPanel::Menu,
            3,
            Action::Next,
            ReaderPanel::Menu,
            5,
            ReaderUiIntent::Repaint,
        ),
        (
            ReaderPanel::Menu,
            5,
            Action::Previous,
            ReaderPanel::Menu,
            3,
            ReaderUiIntent::Repaint,
        ),
        (
            ReaderPanel::Menu,
            4,
            Action::Activate,
            ReaderPanel::Menu,
            5,
            ReaderUiIntent::Home,
        ),
        (
            ReaderPanel::Menu,
            u32::MAX,
            Action::Previous,
            ReaderPanel::Menu,
            3,
            ReaderUiIntent::Repaint,
        ),
        (
            ReaderPanel::Menu,
            1,
            Action::Activate,
            ReaderPanel::Chapters,
            u32::MAX,
            ReaderUiIntent::Repaint,
        ),
        (
            ReaderPanel::Settings,
            3,
            Action::Activate,
            ReaderPanel::Settings,
            3,
            ReaderUiIntent::SaveSettings,
        ),
        (
            ReaderPanel::Settings,
            4,
            Action::Activate,
            ReaderPanel::Menu,
            0,
            ReaderUiIntent::Repaint,
        ),
        (
            ReaderPanel::Statistics,
            1,
            Action::Activate,
            ReaderPanel::Statistics,
            5,
            ReaderUiIntent::Repaint,
        ),
        (
            ReaderPanel::Statistics,
            253,
            Action::Activate,
            ReaderPanel::Statistics,
            1,
            ReaderUiIntent::Repaint,
        ),
        (
            ReaderPanel::Chapters,
            9,
            Action::Activate,
            ReaderPanel::Page,
            1,
            ReaderUiIntent::Repaint,
        ),
    ];
    for (panel, selected, action, want_panel, want_selected, want_intent) in cases {
        let mut ui = ReaderUiState {
            panel,
            selected,
            ..ReaderUiState::default()
        };
        assert_eq!(
            dispatch_reader_ui(&mut ui, settings, action, 2),
            want_intent
        );
        assert_eq!((ui.panel, ui.selected), (want_panel, want_selected));
    }
}
#[test]
fn finish_visibility_and_unwritable_settings() {
    for finish in [ReaderFinishRow::Mark, ReaderFinishRow::Undo] {
        let mut ui = ReaderUiState {
            panel: ReaderPanel::Menu,
            selected: 4,
            finish,
            ..ReaderUiState::default()
        };
        assert_eq!(
            dispatch_reader_ui(&mut ui, ViewSettings::default(), Action::Activate, 0),
            ReaderUiIntent::MarkFinished
        );
        assert_eq!(ui.finish, finish);
    }
    let mut ui = ReaderUiState {
        panel: ReaderPanel::Settings,
        selected: 3,
        preferences_writable: false,
        ..ReaderUiState::default()
    };
    let draft = ui.draft;
    assert_eq!(
        dispatch_reader_ui(&mut ui, ViewSettings::default(), Action::Activate, 0),
        ReaderUiIntent::Repaint
    );
    assert_eq!(ui.notice, ReaderUiNotice::SaveFailed);
    assert_eq!(ui.draft, draft);
    ui.panel = ReaderPanel::Chapters;
    ui.selected = 99;
    assert_eq!(
        dispatch_reader_ui(&mut ui, draft, Action::Next, 0),
        ReaderUiIntent::Repaint
    );
    assert_eq!(ui.selected, 99); // zero-count early return preserves pinned state
}
#[test]
fn notices_and_geometry() {
    for notice in [
        ReaderUiNotice::BookChanged,
        ReaderUiNotice::VerificationTimedOut,
    ] {
        let mut ui = ReaderUiState {
            notice,
            ..ReaderUiState::default()
        };
        dispatch_reader_ui(&mut ui, ViewSettings::default(), Action::Read, 0);
        assert_eq!(ui.notice, ReaderUiNotice::None);
    }
    for (orientation, w, y) in [
        (Orientation::Portrait, 474, 180),
        (Orientation::LandscapeCcw, 637, 64),
        (Orientation::LandscapeCw, 637, 64),
    ] {
        let g = overlay_geometry(ViewSettings {
            orientation,
            ..ViewSettings::default()
        });
        assert_eq!((g.x, g.y, g.width, g.height), (27, y, w, 402));
    }
}

#[test]
fn chapter_next_preserves_requested_focus_for_preparation() {
    let mut ui = ReaderUiState {
        panel: ReaderPanel::Chapters,
        selected: 1,
        ..ReaderUiState::default()
    };
    assert_eq!(
        dispatch_reader_ui(&mut ui, ViewSettings::default(), Action::Next, 6),
        ReaderUiIntent::Repaint
    );
    assert_eq!(ui.selected, 2);
    // Actual OperationReader preparation must now request 2 before paint;
    // immutable composition cannot replace it with the old menu focus of 1.
}

#[test]
fn orientation_row_cycles_every_choice_without_saving() {
    let settings = ViewSettings::default();
    let mut ui = ReaderUiState { panel: ReaderPanel::Settings, draft: settings, ..ReaderUiState::default() };
    for expected in [Orientation::LandscapeCw, Orientation::Portrait, Orientation::LandscapeCcw] {
        assert_eq!(dispatch_reader_ui(&mut ui, settings, Action::Activate, 0), ReaderUiIntent::Repaint);
        assert_eq!(ui.draft.orientation, expected);
    }
}
