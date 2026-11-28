//! #285: restore validated pixels after a scratch loan, including failed work.
use crate::*;
use newt_core::{progress::Progress, xml_session::ParserBackend, zip_container::Status};
use newt_phased_reader::{CachePresence, PhasedReader, phase_storage::ReadScope};
impl<B: ParserBackend + ?Sized, L: ReaderPanelLoan, C: ReadScope + CachePresence>
    PhysicalOwner<'_, '_, '_, B, L, C>
{
    /// This restores pixels only; it neither certifies the source nor clears
    /// an owner failure, resumes tracking, or publishes a display refresh.
    pub fn restore_retained_native(
        &mut self, reader: &PhasedReader<'_, '_>, owner: &ExternalOwner,
        ui: &ReaderUiState, settings: ViewSettings, info: &DeviceInfo<'_>,
        progress: &Progress, book: Option<&BookStatsView>,
    ) -> Result<(), Status> {
        let status = self.panel.with_frame(|frame| reader.restore_retained_frame(frame))?;
        if status != Status::Ok { return Err(status); }
        self.compose_native(reader, owner, ui, settings, info, progress,
            PhysicalPaint(Painted::Drawn { ready: true }), book).map(|_| ())
    }
}
