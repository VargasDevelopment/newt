use newt_core::durable::*;
use std::cell::Cell;

struct ReadOnly<'a> {
    opened: &'a Cell<bool>,
    slots: [[u8; 20]; 2],
}

impl Backend for ReadOnly<'_> {
    fn begin(&mut self, _: &str, readonly: bool) -> StorageOpen {
        assert!(readonly);
        assert!(!self.opened.replace(true));
        StorageOpen::Opened
    }
    fn end(&mut self) {
        assert!(self.opened.replace(false));
    }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        assert!(self.opened.get());
        output.copy_from_slice(&self.slots[usize::from(key == "slot1")]);
        BlobRead::Present(20)
    }
    fn put_bytes(&mut self, _: DurableWrite, _: &str, _: &[u8]) -> usize {
        panic!("read-only test")
    }
    fn readback_durable(&self) -> bool {
        true
    }
}

#[test]
fn matching_callbacks_run_after_the_backend_transaction_closes() {
    let opened = Cell::new(false);
    let mut backend = ReadOnly {
        opened: &opened,
        slots: [[0; 20]; 2],
    };
    assert!(TimeZoneCodec::encode(1, 1, &mut backend.slots[0]));
    assert!(TimeZoneCodec::encode(1, 2, &mut backend.slots[1]));
    let matches = Cell::new(0);
    let equivalents = Cell::new(0);
    let result = RecordStore::<_, TimeZoneCodec, 20>::new(&mut backend).load_matching(
        |_| {
            assert!(!opened.get());
            matches.set(matches.get() + 1);
            true
        },
        |a, b| {
            assert!(!opened.get());
            equivalents.set(equivalents.get() + 1);
            a == b
        },
    );
    assert_eq!(result, Load::Loaded(1));
    assert_eq!(matches.get(), 2);
    assert_eq!(equivalents.get(), 1);
}

#[test]
fn admission_stays_entered_after_quiet_window_changes() {
    let quiet_now = Cell::new(false);
    let mut quiet = || quiet_now.get();
    let mut admission = Admission {
        quiet: Some(&mut quiet),
        entered: false,
        deferred: false,
    };
    assert!(!admission.enter());
    assert!(admission.deferred);
    quiet_now.set(true);
    assert!(admission.enter());
    quiet_now.set(false);
    assert!(admission.ready());
    assert!(admission.enter());
}
