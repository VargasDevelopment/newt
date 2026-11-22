//! Runtime-borrowed Library seam; retention only, never target execution.
use newt_library_ui::*;
/// Receives the actual caller-owned catalog, logical/native-sized frame, state
/// and cover source. No replacement catalog, cache, or frame is constructed.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn library_ui_borrowed(
    catalog: Option<&LibraryCatalog>,
    state: &mut LibraryUiState<'_>,
    frame: Option<&mut Frame>,
    bar: &SystemBar,
    covers: Option<&mut dyn LibraryCoverSource>,
    action: Action,
) -> (LibraryIntent, bool) {
    let intent = dispatch(state, catalog.map_or(0, LibraryCatalog::size), action);
    (intent, paint(catalog, state, frame, bar, covers))
}
