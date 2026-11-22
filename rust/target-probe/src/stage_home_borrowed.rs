//! Normal staticlib entry: all runtime state and image storage stay borrowed.
use newt_stage_home::*;

#[unsafe(no_mangle)]
#[inline(never)]
pub fn stage_home_borrowed(
    state: &AppState,
    info: &HeaderInfo,
    data: &HomeData<'_>,
    plane: HomePlane,
    frame: &mut Frame,
    backdrop: &StageBackdrop,
    cover: Option<&PreparedHomeCover<'_>>,
    focus: HomeFocusStyle,
    words: &mut HomeText,
) -> u32 {
    render_stage_home_focused(
        state,
        info,
        data,
        plane,
        Some(&mut *frame),
        backdrop,
        cover,
        focus,
    );
    *words = stage_text(data);
    frame
        .iter()
        .chain(words.title.iter())
        .chain(words.progress.iter())
        .chain(words.hint.iter())
        .fold(0u32, |sum, byte| sum.rotate_left(1) ^ u32::from(*byte))
}
