//! Exercise the firmware's actual source-state guard, not a substitute loan.
#[path = "../../firmware/reader_image_source_loan.rs"]
mod source_loan;
use core::cell::Cell;
use newt_core::zip_window::source::BookSource;
use newt_phased_reader::imported_revision::{
    Digest, ImportedRevision, RevisionHasher, RevisionPorts, RevisionStorage,
};
use newt_reader_ui::ReaderSource;

struct Source<'a>(&'a Cell<u32>);
impl BookSource for Source<'_> {
    fn size(&self) -> u64 { 17 }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if !out.is_empty() { self.0.set(self.0.get() + 1); }
        let count = out.len().min(17u64.saturating_sub(at) as usize);
        out[..count].fill(0x31);
        count as i32
    }
}
struct Store { generation: u64, bytes: [u8; 160] }
impl RevisionStorage for Store {
    fn load(&mut self, bytes: &mut u32) -> bool { *bytes = 160; true }
    fn begin(&mut self, bytes: u32) -> bool { self.generation += 1; bytes == 160 }
    fn read(&mut self, at: u32, out: &mut [u8]) -> bool {
        let at = at as usize;
        let Some(input) = self.bytes.get(at..at.saturating_add(out.len())) else { return false; };
        out.copy_from_slice(input); true
    }
    fn write(&mut self, at: u32, input: &[u8]) -> bool {
        let at = at as usize;
        let Some(out) = self.bytes.get_mut(at..at.saturating_add(input.len())) else { return false; };
        out.copy_from_slice(input); true
    }
    fn seal(&mut self) -> bool { true }
    fn abort(&mut self) { self.generation += 1; }
    fn generation(&self) -> u64 { self.generation }
    fn stable(&mut self, generation: u64) -> bool { generation == self.generation }
}
struct Hash;
impl RevisionHasher for Hash {
    fn begin(&mut self) -> bool { true }
    fn update(&mut self, _: &[u8]) -> bool { true }
    fn finish(&mut self, out: &mut Digest) -> bool { *out = [0xa7; 32]; true }
    fn hash(&mut self, _: &[u8], out: &mut Digest) -> bool { *out = [0xa7; 32]; true }
}

#[derive(Clone, Copy)]
enum Exit { Success, Error, Unwind }

fn exercise(exit: Exit) {
    let reads = Cell::new(0);
    let mut raw = Source(&reads);
    let mut store = Store { generation: 7, bytes: [0; 160] };
    let mut whole = Hash;
    let mut block = Hash;
    let mut owned = [0; 256];
    let mut action = [0x99; 52_272];
    let mut revision = ImportedRevision::compact(&mut store, &mut whole, &mut block, &mut owned);
    assert!(revision.begin_import(&mut raw));
    let mut slot = Some(ReaderSource::new(revision).into_state());
    let before = reads.get();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        source_loan::with_source_lent(&mut slot, RevisionPorts {
            source: Some(&mut raw), store: &mut store, whole: &mut whole,
            block: Some(&mut block), scratch: &mut owned,
        }, Some(&mut action), |source| {
            // One authenticated block is retained only during this operation.
            let mut bytes = [0; 17];
            assert_eq!(source.read_at(0, &mut bytes), 17);
            assert_eq!(bytes, [0x31; 17]);
            let first = reads.get();
            assert!(first > before);
            assert_eq!(source.read_at(0, &mut bytes), 17);
            assert_eq!(reads.get(), first);
            match exit {
                Exit::Success => Ok(()),
                Exit::Error => Err("operation failed"),
                Exit::Unwind => panic!("operation unwind"),
            }
        })
    }));
    match exit {
        Exit::Success => assert_eq!(result.expect("ordinary return"), Ok(Ok(()))),
        Exit::Error => assert_eq!(result.expect("typed error return"), Ok(Err("operation failed"))),
        Exit::Unwind => assert!(result.is_err()),
    }
    assert!(slot.is_some(), "actual resulting state returned on every exit");
    // The frame may be reused now. Relending the original 256-byte slice must
    // succeed and authenticate afresh; it cannot use the old retained tag.
    action.fill(0x7d);
    let before = reads.get();
    let restored = source_loan::with_source(&mut slot, RevisionPorts {
        source: Some(&mut raw), store: &mut store, whole: &mut whole,
        block: Some(&mut block), scratch: &mut owned,
    }, |source| {
        let mut bytes = [0; 17];
        assert_eq!(source.read_at(0, &mut bytes), 17);
        assert_eq!(bytes, [0x31; 17]);
    });
    assert!(restored.is_ok(), "original scratch shape restored before state return");
    assert!(reads.get() > before, "a new operation authenticates afresh");
}

#[test]
fn success_restores_original_scratch() { exercise(Exit::Success); }
#[test]
fn typed_error_restores_original_scratch() { exercise(Exit::Error); }
#[test]
fn unwind_restores_original_scratch() { exercise(Exit::Unwind); }

#[test]
fn refused_small_loan_preserves_state_without_running_the_operation() {
    let reads = Cell::new(0);
    let mut raw = Source(&reads);
    let mut store = Store { generation: 7, bytes: [0; 160] };
    let mut whole = Hash;
    let mut block = Hash;
    let mut owned = [0; 256];
    let mut action = [0x99; 4095];
    let mut revision = ImportedRevision::compact(&mut store, &mut whole, &mut block, &mut owned);
    assert!(revision.begin_import(&mut raw));
    let mut slot = Some(ReaderSource::new(revision).into_state());
    let before = reads.get();
    let result = source_loan::with_source_lent(&mut slot, RevisionPorts {
        source: Some(&mut raw), store: &mut store, whole: &mut whole,
        block: Some(&mut block), scratch: &mut owned,
    }, Some(&mut action), |_| panic!("refused loan cannot run the operation"));
    assert_eq!(result, Err(newt_phased_reader::imported_revision::StateLoanError::Shape));
    assert_eq!(reads.get(), before, "lending itself performs no source I/O");
    assert_eq!(action, [0x99; 4095]);
    assert!(source_loan::with_source(&mut slot, RevisionPorts {
        source: Some(&mut raw), store: &mut store, whole: &mut whole,
        block: Some(&mut block), scratch: &mut owned,
    }, |_| {}).is_ok());
}

// The loan fixture uses paint facts; the other fields serve the app loop.
#[allow(dead_code)]
#[path = "../../firmware/app_runtime/reader_facts.rs"]
mod actual_reader_facts;

#[test]
fn changed_source_reopen_never_publishes_lent_pixels() {
    use newt_core::{app::Screen, frame::FRAME_BYTES};
    let pixels = [0x7d; FRAME_BYTES];
    for screen in [Screen::Home, Screen::EpubReading] {
        for opened in [false, true] {
            for reopen_owed in [false, true] {
                let facts = actual_reader_facts::ReaderFacts { status: 0, opened,
                    progress_ready: true, spine: 0, character: 0, book: 7, reopen_owed };
                let deferred = facts.defers_paint(screen);
                assert_eq!(facts.frame(screen, &pixels).is_none(), deferred);
                if screen == Screen::EpubReading && !opened && reopen_owed {
                    assert!(deferred, "changed reopen retains the paper image, not logical scratch");
                } else {
                    assert!(!deferred, "completed/current frames remain publishable");
                }
            }
        }
    }
}
