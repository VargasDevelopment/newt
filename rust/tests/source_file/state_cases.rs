//! The new binding-state seam only. Original FileBookSource read/bind policy
//! is unchanged and retains its separate original-C++ differential obligations.
use super::*;
use core::cell::Cell;

struct Calls { opened: Cell<u32>, directory: Cell<u32>, size: Cell<u32>, reads: Cell<u32> }
impl Calls {
    fn new() -> Self {
        Self { opened: Cell::new(0), directory: Cell::new(0), size: Cell::new(0), reads: Cell::new(0) }
    }
    fn observed(&self) -> [u32; 4] {
        [self.opened.get(), self.directory.get(), self.size.get(), self.reads.get()]
    }
}
fn tick(value: &Cell<u32>) { value.set(value.get() + 1); }
struct Input<'a> { calls: &'a Calls, opened: bool, directory: bool, bytes: u64, position: u64 }
impl<'a> Input<'a> {
    fn new(calls: &'a Calls) -> Self {
        Self { calls, opened: true, directory: false, bytes: 17, position: 0 }
    }
}
impl File for Input<'_> {
    fn opened(&mut self) -> bool { tick(&self.calls.opened); self.opened }
    fn is_directory(&mut self) -> bool { tick(&self.calls.directory); self.directory }
    fn size(&mut self) -> u64 { tick(&self.calls.size); self.bytes }
    fn position(&mut self) -> u64 { self.position }
    fn seek(&mut self, at: u32) -> bool { self.position = u64::from(at); true }
    fn read(&mut self, output: &mut [u8]) -> usize {
        tick(&self.calls.reads); output.fill(0x31); self.position += output.len() as u64; output.len()
    }
}
struct Cancel(Cell<bool>);
impl Authority for Cancel { fn cancelled(&self) -> bool { self.0.get() } }

#[test]
fn state_reloans_the_actual_binding_without_io_or_rebind() {
    let calls = Calls::new();
    let mut file = Input::new(&calls);
    let address = &file as *const Input<'_>;
    let mut source = FileBookSource::new(None);
    assert!(source.bind(&mut file));
    assert_eq!(calls.observed(), [1, 1, 1, 0]);
    let (state, original) = match source.into_state() {
        Ok(result) => result, Err(_) => panic!("actual bound source"),
    };
    assert!(core::ptr::eq(original, address));
    let mut source = state.lend(original, None);
    assert_eq!(source.size(), 17);
    assert_eq!(calls.observed(), [1, 1, 1, 0]);
    assert_eq!(source.read_at(0, &mut [0; 4]), 4);
    assert_eq!(calls.observed(), [2, 1, 2, 1]);
    let (state, original) = match source.into_state() {
        Ok(result) => result, Err(_) => panic!("read result retains binding"),
    };
    assert_eq!(state.lend(original, None).size(), 17);
    assert_eq!(calls.observed(), [2, 1, 2, 1]);
    assert_eq!(core::mem::size_of::<SourceState>(), 8);
    assert_eq!(core::mem::size_of::<Option<SourceState>>(), 8);
}

#[test]
fn relending_does_not_accept_a_changed_size() {
    let calls = Calls::new();
    let mut file = Input::new(&calls);
    let mut source = FileBookSource::new(None);
    assert!(source.bind(&mut file));
    let (state, original) = match source.into_state() {
        Ok(result) => result, Err(_) => panic!("actual bound source"),
    };
    original.bytes = 18;
    let mut source = state.lend(original, None);
    assert_eq!(source.size(), 17);
    assert_eq!(calls.observed(), [1, 1, 1, 0]);
    assert_eq!(source.read_at(0, &mut []), -1);
    assert_eq!(calls.observed(), [2, 1, 2, 0]);
    let (state, original) = match source.into_state() {
        Ok(result) => result, Err(_) => panic!("failed read retains old binding"),
    };
    assert_eq!(state.lend(original, None).size(), 17);
}

#[test]
fn cancellation_is_a_short_live_loan_not_retained_state() {
    let calls = Calls::new();
    let cancel = Cancel(Cell::new(false));
    let mut file = Input::new(&calls);
    let mut source = FileBookSource::new(Some(&cancel));
    assert!(source.bind(&mut file));
    let (state, original) = match source.into_state() {
        Ok(result) => result, Err(_) => panic!("actual bound source"),
    };
    cancel.0.set(true);
    let mut source = state.lend(original, Some(&cancel));
    assert_eq!(source.read_at(0, &mut []), -1);
    assert_eq!(calls.observed(), [1, 1, 1, 0]);
    let (state, original) = match source.into_state() {
        Ok(result) => result, Err(_) => panic!("cancelled read retains binding"),
    };
    cancel.0.set(false);
    let mut source = state.lend(original, Some(&cancel));
    assert_eq!(source.read_at(0, &mut []), 0);
    assert_eq!(calls.observed(), [2, 1, 2, 0]);
}

#[test]
fn only_a_successful_original_bind_mints_state() {
    for fault in 0..5 {
        let calls = Calls::new();
        let mut file = Input::new(&calls);
        file.opened = fault != 0;
        file.directory = fault == 1;
        file.bytes = if fault == 2 { 0 } else if fault == 3 { MAX_BOOK_BYTES + 1 } else { 17 };
        let cancel = Cancel(Cell::new(fault == 4));
        let mut source = FileBookSource::new(Some(&cancel));
        assert!(!source.bind(&mut file));
        assert_eq!(source.size(), 0);
        assert!(source.into_state().is_err());
    }
}

#[test]
fn a_closed_original_file_is_not_reopened_by_lend() {
    let calls = Calls::new();
    let mut file = Input::new(&calls);
    let mut source = FileBookSource::new(None);
    assert!(source.bind(&mut file));
    let (state, original) = match source.into_state() {
        Ok(result) => result, Err(_) => panic!("actual bound source"),
    };
    original.opened = false;
    let mut source = state.lend(original, None);
    assert_eq!(source.size(), 17);
    assert_eq!(source.read_at(0, &mut []), -1);
    assert_eq!(calls.observed(), [2, 1, 1, 0]);
}
