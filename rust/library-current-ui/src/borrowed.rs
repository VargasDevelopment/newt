//! MIT. Source-only RV32 retention proposal, not executed or retained evidence.
use crate::*;
use newt_library_current::list::Files;
/// Borrow the real caller's catalog, files, frame, and source. No owned frame,
/// catalog table, cover cache, or fallible scratch allocation is introduced.
#[inline(never)]
pub fn alpha15_library_ui_borrowed(
    catalog: Option<&mut Catalog>,
    files: &mut dyn Files,
    state: &mut LibraryUiState<'_>,
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    covers: Option<&mut dyn LibraryCoverSource>,
    action: Action,
) -> (LibraryIntent, bool) {
    let intent = dispatch(state, catalog.as_deref().map_or(0, Catalog::size), action);
    (intent, paint(catalog, files, state, frame, bar, covers))
}
