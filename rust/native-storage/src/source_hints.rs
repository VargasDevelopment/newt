//! Optional verified-source change hints storage. MIT.
//! Child of port: shares the actual native owner, not a second filesystem loan.
use super::{FilePort, Native, Operation, Route, FilePortState, LoanError, CloseOutcome};

pub use newt_core::reader_source_hints::{Digest, SourceChangeHints, WIRE_BYTES};
use newt_core::reader_source_hints as policy;

/// Persistent optional-hint state, with its original one-time name result.
#[must_use = "explicitly close hints at the enclosing source boundary"]
pub struct ReaderSourceHintsState {
    file: FilePortState,
    named: bool,
}
impl ReaderSourceHintsState {
    pub fn new<S: Native>(native: &mut S, route: &Route<'_>) -> Self {
        let (file, named) = FilePortState::new_named(native, route, "Imported", "source-hints", true);
        Self { file, named }
    }
    pub fn close<S: Native>(
        &mut self, native: &mut S, route: &Route<'_>,
    ) -> core::result::Result<CloseOutcome, LoanError> {
        self.file.close(native, route, "Imported")
    }
}
/// A hint never authorizes bytes: full-digest verification remains required.
pub struct ReaderSourceHints<'a, S: Native> {
    file: FilePort<'a, S>,
    named: bool,
}
impl<'a, S: Native> ReaderSourceHints<'a, S> {
    pub fn lend(
        state: &'a mut ReaderSourceHintsState, native: &'a mut S, route: &'a Route<'a>,
    ) -> core::result::Result<Self, LoanError> {
        Ok(Self { file: FilePort::lend(&mut state.file, native, route, "Imported")?, named: state.named })
    }
    pub fn load(&mut self, expected: &Digest, out: &mut SourceChangeHints) -> bool {
        self.read_record(Some(expected), None, out)
    }
    pub fn recorded(&mut self, digest: &mut Digest, out: &mut SourceChangeHints) -> bool {
        self.read_record(None, Some(digest), out)
    }
    pub fn verified_source_digest(&mut self, now: SourceChangeHints, digest: &mut Digest) -> bool {
        let mut recorded = SourceChangeHints::default();
        let recorded_ok = self.recorded(digest, &mut recorded);
        policy::verified_source_digest(recorded_ok, recorded, now)
    }
    pub fn save(&mut self, digest: &Digest, hints: SourceChangeHints) -> bool {
        if !hints.valid() {
            return false;
        }
        let mut wire = [0; WIRE_BYTES];
        if !policy::encode(digest, hints, &mut wire) {
            return false;
        }
        let mut file = self.file.operation();
        let lease = file.release();
        if file.generation() != lease
            || !self.named
            || !file.begin(WIRE_BYTES as u32, true)
            || !file.write(0, &wire, false)
            || !file.sync()
        {
            return close(&mut file, false);
        }
        let mut readback = [0; WIRE_BYTES];
        if !file.read(0, &mut readback) || readback != wire || !file.commit(WIRE_BYTES as u32, true)
        {
            return close(&mut file, false);
        }
        close(&mut file, true)
    }
    pub fn remove(&mut self) -> bool {
        self.file.operation().release();
        self.named && self.file.route.remove_record(self.file.native, "Imported", "source-hints", true)
    }
    fn read_record(
        &mut self,
        expected: Option<&Digest>,
        digest: Option<&mut Digest>,
        out: &mut SourceChangeHints,
    ) -> bool {
        *out = SourceChangeHints::default();
        let mut file = self.file.operation();
        let lease = file.release();
        if file.generation() != lease
            || !self.named
            || !file.open_read(WIRE_BYTES as u32)
            || file.bytes() != WIRE_BYTES as u32
        {
            return close(&mut file, false);
        }
        let mut wire = [0; WIRE_BYTES];
        file.begin_immutable_read();
        let mut staged = SourceChangeHints::default();
        let decoded = file.read(0, &mut wire)
            && policy::decode_into(Some(&wire), WIRE_BYTES as i64, expected, digest, &mut staged);
        let fresh = file.finish_immutable_read();
        if !close(&mut file, decoded && fresh) {
            return false;
        }
        *out = staged;
        true
    }
}
fn close<S: Native>(file: &mut Operation<'_, '_, S>, result: bool) -> bool {
    let released = file.release();
    result && file.released_cleanly(released)
}
