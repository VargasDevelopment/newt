//! Unit-only reservation failure scenarios. Source only: not compiled or run.
include!("../../tests/scenarios.rs");

std::thread_local! {
    static RESERVATIONS: std::cell::Cell<(usize, usize)> = const {
        std::cell::Cell::new((0, 0))
    };
}

pub(crate) fn request(bytes: usize) -> usize {
    RESERVATIONS.with(|state| {
        let (fail, calls) = state.get();
        state.set((fail, calls + 1));
        // Force the actual Vec reservation to fail without unsafe allocator
        // hooks. This is capacity exhaustion, not measured physical heap OOM.
        if fail == calls + 1 { usize::MAX } else { bytes }
    })
}

fn fail_at(ordinal: usize) {
    RESERVATIONS.with(|state| state.set((ordinal, 0)));
}

fn calls() -> usize {
    RESERVATIONS.with(|state| state.get().1)
}

#[test]
fn default_reset_and_list_release_the_table_storage() {
    let mut c = Catalog::default();
    assert_eq!(
        (c.table.len(), c.table.capacity(), c.window.capacity()),
        (0, 0, 0)
    );
    c.add(b"a.epub\0", 1, None, true);
    assert_eq!(c.table.len(), 12800);
    c.finish();
    c.use_list(b"/list\0", 1, 0);
    assert_eq!(
        (c.table.len(), c.table.capacity(), c.table_count),
        (0, 0, 0)
    );
    c.reset(LibraryStatus::Ok);
    assert_eq!((c.table.capacity(), c.window.capacity()), (0, 0));
}

#[test]
fn walk_and_table_reservation_failures_publish_no_stale_catalog() {
    for ordinal in [1, 2] {
        let mut c = Catalog::default();
        c.add(b"old.epub\0", 1, None, true);
        c.finish();
        let mut f = Card::default();
        let mut folders = Root {
            count: 3,
            at: 0,
            failed: false,
        };
        let mut r = Report::default();
        fail_at(ordinal);
        assert_eq!(
            scan(
                b"/card\0",
                &mut c,
                &mut r,
                &mut folders,
                &mut f,
                None,
                &mut NoProgress,
                false
            ),
            LibraryStatus::Unavailable
        );
        fail_at(0);
        assert_eq!(c.size(), 0);
        assert_eq!((c.table.capacity(), c.window.capacity()), (0, 0));
        assert_eq!(r.walks, 1);
        assert_eq!(r.list, ListState::None);
        if ordinal == 1 {
            assert_eq!(r.entries, 0);
            assert_eq!(r.folders, 0);
        }
    }
}

#[test]
fn build_reservation_failure_keeps_small_ram_or_rescans_overflow() {
    for (count, walks, reservations) in [(3, 1, 3), (259, 2, 5)] {
        let mut c = Catalog::default();
        let mut f = Card::default();
        let mut folders = Root {
            count,
            at: 0,
            failed: false,
        };
        let mut r = Report::default();
        fail_at(3); // Walk, table, then Build's real path buffer.
        assert_eq!(
            scan(
                b"/card\0",
                &mut c,
                &mut r,
                &mut folders,
                &mut f,
                None,
                &mut NoProgress,
                false
            ),
            LibraryStatus::Ok
        );
        assert_eq!(calls(), reservations);
        fail_at(0);
        assert_eq!(r.list, ListState::Ram);
        assert_eq!(r.walks, walks);
        assert_eq!(c.size(), count.min(128));
        assert!(!c.on_card());
        assert!(f.paths.is_empty());
        assert_eq!(c.filename(&mut f, 0), Some(b":00001.epub\0".as_slice()));
    }
}

#[test]
fn ram_mode_never_reserves_build_and_replacement_build_can_fail() {
    for (ram, failure, walks, reservations, status) in [
        (true, 3, 1, 2, LibraryStatus::Ok),
        (true, 5, 2, 4, LibraryStatus::Ok),
        (false, 6, 3, 8, LibraryStatus::Ok),
        (false, 4, 2, 4, LibraryStatus::Unavailable),
        (false, 7, 3, 7, LibraryStatus::Unavailable),
    ] {
        let count = if ram && failure == 3 { 3 } else { 259 };
        let mut c = Catalog::default();
        let mut f = Card::default();
        f.fail_write = failure == 7; // Force fallback after the replacement Build.
        let mut folders = Root {
            count,
            at: 0,
            failed: false,
        };
        let mut r = Report::default();
        fail_at(failure);
        assert_eq!(
            scan(
                b"/card\0",
                &mut c,
                &mut r,
                &mut folders,
                &mut f,
                None,
                &mut NoProgress,
                ram
            ),
            status
        );
        assert_eq!(calls(), reservations);
        fail_at(0);
        assert_eq!(r.walks, walks);
        assert!(!c.on_card());
        assert!(f.fds.is_empty());
        if status == LibraryStatus::Ok {
            assert_eq!(r.list, ListState::Ram);
            assert_eq!(c.size(), count.min(128));
        } else {
            assert_eq!(c.size(), 0);
            assert_eq!((c.table.capacity(), c.window.capacity()), (0, 0));
            assert_eq!(r.entries, count as u32); // Failed Walk keeps prior telemetry.
            assert_eq!(r.folders, 1);
            assert_eq!(
                r.list,
                if failure == 4 {
                    ListState::None
                } else {
                    ListState::Ram
                }
            );
        }
    }
}
