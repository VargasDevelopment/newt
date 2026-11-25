//! Reader failure and position policy over the actual owner state.
use super::*;
#[test]
fn failures_latch_independently_of_retained_progress() {
    let mut owner = ReaderOwnerState::from_open(true, Status::Ok, 4);
    owner.note_failure(Status::IoError);
    assert!(owner.has_progress());
    assert!(!owner.opened());
    owner.menu_result(Status::Ok);
    owner.note_failure(Status::Ok);
    assert_eq!(owner.status(), Status::IoError);
    assert!(!owner.opened());
    assert!(!ReaderOwnerState::from_open(false, Status::Ok, 4).opened());
}
#[test]
fn only_menu_workspace_failures_are_optional() {
    for status in [Status::Unsupported, Status::OutOfMemory] {
        let mut owner = ReaderOwnerState::from_open(true, Status::Ok, 5);
        owner.menu_result(status);
        assert_eq!(owner.status(), Status::Ok);
        assert!(owner.opened());
    }
    for status in [
        Status::IoError,
        Status::Truncated,
        Status::ParseError,
        Status::Stale,
        Status::NotFound,
        Status::NotZip,
        Status::NotEpub,
        Status::Encrypted,
        Status::Damaged,
    ] {
        let mut owner = ReaderOwnerState::from_open(true, Status::Ok, 5);
        owner.menu_result(status);
        assert_eq!(owner.status(), status);
        assert!(!owner.opened());
        assert!(owner.has_progress());
    }
    let mut owner = ReaderOwnerState::from_open(true, Status::Ok, 5);
    owner.note_failure(Status::OutOfMemory); // final PAINT failure is consequential
    assert!(!owner.opened());
}
#[test]
fn a_damaged_chapter_shows_the_chapter_notice_like_a_read_failure() {
    let notice = |status| ReaderOwnerState::from_open(true, status, 5).chapter_notice();
    assert!(notice(Status::Damaged));
    assert!(notice(Status::IoError));
    // Book-level and workspace failures are not the chapter's own.
    assert!(!notice(Status::Ok));
    assert!(!notice(Status::OutOfMemory));
    assert!(!notice(Status::Stale));
}
#[test]
fn readable_bound_is_the_single_last_page_authority() {
    let owner = ReaderOwnerState::from_open(true, Status::Ok, 3);
    let raw = PageInfo {
        spine: 2,
        page: 4,
        page_count: 5,
        char_start: 80,
        overall_known: true,
        overall_percent: 63,
        ..PageInfo::default()
    };
    let position = owner.project(raw, 7);
    assert_eq!(position.page.overall_percent, 100);
    assert!(position.shown.last);
    assert_eq!(
        position.shown.locator,
        Locator {
            spine: 2,
            character: 80
        }
    );
    assert_eq!(raw.overall_percent, 63);
    // Literal non-last, raw bound smaller, unknown, empty-page counterexamples.
    assert_eq!(
        owner
            .project(PageInfo { spine: 1, ..raw }, 7)
            .page
            .overall_percent,
        63
    );
    assert_eq!(
        owner
            .project(PageInfo { page: 3, ..raw }, 7)
            .page
            .overall_percent,
        63
    );
    assert!(owner.project(PageInfo { spine: 1, ..raw }, 2).shown.last);
    let unknown = owner.project(
        PageInfo {
            overall_known: false,
            ..raw
        },
        7,
    );
    assert_eq!(unknown.page.overall_percent, 63);
    assert!(unknown.shown.last);
    assert!(
        !owner
            .project(
                PageInfo {
                    page_count: 0,
                    ..raw
                },
                7
            )
            .shown
            .last
    );
}

#[test]
fn retained_projection_keeps_the_has_progress_guard() {
    let stale = PageInfo { spine: 2, page: 4, page_count: 5, char_start: 80,
        overall_known: true, overall_percent: 63, ..PageInfo::default() };
    let closed = ReaderOwnerState::from_open(false, Status::IoError, 3);
    assert_eq!(closed.position_info(stale, 7).page, PageInfo::default());
    let retained = ReaderOwnerState::from_open(true, Status::IoError, 3);
    let position = retained.position_info(stale, 7);
    assert_eq!(position.page.char_start, 80);
    assert_eq!(position.page.overall_percent, 100);
    assert!(position.shown.last);
    assert!(!retained.opened());
}
