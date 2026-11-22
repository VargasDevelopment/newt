//! MIT. SD-backed `RevisionStorage` without a second file owner.
use newt_native_storage::{FilePort, Native, Route, FilePortState, LoanError, CloseOutcome};
use newt_phased_reader::imported_revision::{MAX_RECEIPT_BYTES, RevisionStorage};

/// Persistent receipt state at the optional reader-session boundary.
#[must_use = "explicitly close the receipt at the enclosing source boundary"]
pub struct SdRevisionState {
    file: FilePortState,
    named: bool,
}
impl SdRevisionState {
    pub fn new<N: Native>(native: &mut N, route: &Route<'_>) -> Self {
        let (file, named) = FilePortState::new_named(native, route, "Imported", "receipt", true);
        Self { file, named }
    }
    pub fn close<N: Native>(
        &mut self, native: &mut N, route: &Route<'_>,
    ) -> core::result::Result<CloseOutcome, LoanError> {
        self.file.close(native, route, "Imported")
    }
}
/// Freshness performs native I/O through this exclusive operation loan.
pub struct SdRevisionStorage<'a, N: Native> {
    file: FilePort<'a, N>,
    named: bool,
}
impl<'a, N: Native> SdRevisionStorage<'a, N> {
    pub fn lend(
        state: &'a mut SdRevisionState, native: &'a mut N, route: &'a Route<'a>,
    ) -> core::result::Result<Self, LoanError> {
        Ok(Self { file: FilePort::lend(&mut state.file, native, route, "Imported")?, named: state.named })
    }
    pub fn close(&mut self) {
        self.file.operation().release();
    }
    pub fn begin_deferred_path_checks(&mut self) {
        self.file.operation().begin_deferred_path_checks();
    }
    pub fn finish_deferred_path_checks(&mut self) -> bool {
        self.file.operation().finish_deferred_path_checks()
    }
    pub fn begin_immutable_read(&mut self) {
        self.file.operation().begin_immutable_read();
    }
    pub fn finish_immutable_read(&mut self) -> bool {
        self.file.operation().finish_immutable_read()
    }
}
impl<N: Native> RevisionStorage for SdRevisionStorage<'_, N> {
    fn begin_immutable_read(&mut self) {
        SdRevisionStorage::begin_immutable_read(self);
    }
    fn finish_immutable_read(&mut self) -> bool {
        SdRevisionStorage::finish_immutable_read(self)
    }
    fn begin_deferred_path_checks(&mut self) {
        SdRevisionStorage::begin_deferred_path_checks(self);
    }
    fn finish_deferred_path_checks(&mut self) -> bool {
        SdRevisionStorage::finish_deferred_path_checks(self)
    }

    fn load(&mut self, bytes: &mut u32) -> bool {
        *bytes = 0;
        let mut file = self.file.operation();
        let lease = file.release();
        if file.generation() != lease {
            return false;
        }
        if !self.named || !file.open_read(MAX_RECEIPT_BYTES) || file.bytes() == 0 {
            if file.generation() == lease {
                file.fail();
            }
            return false;
        }
        *bytes = file.bytes();
        true
    }
    fn begin(&mut self, bytes: u32) -> bool {
        let mut file = self.file.operation();
        let lease = file.release();
        file.generation() == lease
            && self.named
            && bytes != 0
            && bytes <= MAX_RECEIPT_BYTES
            && file.begin(bytes, true)
    }
    fn read(&mut self, offset: u32, output: &mut [u8]) -> bool {
        self.file.operation().read(offset, output)
    }
    fn write(&mut self, offset: u32, input: &[u8]) -> bool {
        let mut file = self.file.operation();
        if offset > file.bytes() || input.len() > (file.bytes() - offset) as usize {
            return file.poison();
        }
        file.write(offset, input, false)
    }
    fn seal(&mut self) -> bool {
        let mut file = self.file.operation();
        file.sync() && file.commit(MAX_RECEIPT_BYTES, true)
    }
    fn abort(&mut self) {
        self.close();
    }
    fn generation(&self) -> u64 {
        self.file.generation()
    }
    fn stable(&mut self, generation: u64) -> bool {
        self.file.operation().stable(generation)
    }
    fn begin_read(&mut self, generation: u64) -> u64 {
        self.file.operation().begin_read(generation)
    }
    fn stable_read(&mut self, generation: u64, token: u64) -> bool {
        self.file.operation().stable_read(generation, token)
    }
    fn read_within(&mut self, generation: u64, token: u64, offset: u32, output: &mut [u8]) -> bool {
        self.file
            .operation()
            .read_within(generation, token, offset, output)
    }
    fn end_read(&mut self, generation: u64, token: u64) -> bool {
        self.file.operation().end_read(generation, token)
    }
    fn cancel_read(&mut self, generation: u64, token: u64) {
        self.file.operation().cancel_read(generation, token);
    }
}
