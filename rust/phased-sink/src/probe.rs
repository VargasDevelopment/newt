//! Borrowed runtime bodies for root's RV32 retention/stack measurement harness.
use super::*;
use newt_core::chapter_layout::types::Backend;
#[inline(never)]
pub fn selected_page_runtime(
    sink: &mut SelectingSink<'_, '_>,
    page: Page<'_>,
    hash: u32,
    start: u32,
) -> (bool, State) {
    fn actual_backend<T: Backend>(backend: &mut T, page: Page<'_>, hash: u32, start: u32) -> bool {
        backend.anchor(hash, start);
        backend.page(page)
    }
    let keep = actual_backend(sink, page, hash, start);
    (keep, sink.state)
}
#[inline(never)]
pub fn repaint_runtime(sink: &mut SelectingSink<'_, '_>, frame: &mut Frame) -> u32 {
    sink.paint_captured(frame);
    frame.iter().fold(0u32, |sum, byte| {
        sum.wrapping_mul(33).wrapping_add(u32::from(*byte))
    })
}
#[inline(never)]
pub fn restart_runtime(sink: &mut SelectingSink<'_, '_>) -> State {
    sink.restart();
    sink.state
}

/// Real chapter-layout connection with all workspace borrowed from the caller.
/// This entry feeds extracted chapter text; XML/session driving can instead
/// construct the same Engine with SelectingSink and LayoutHandler directly.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn chapter_text_runtime<'a>(
    sink: &'a mut SelectingSink<'_, '_>,
    params: newt_core::chapter_layout::types::Params<'a>,
    storage: newt_core::chapter_layout::types::Storage<'a>,
    breaker: &'a mut dyn newt_core::chapter_layout::types::LineBreak,
    images: &'a mut dyn newt_core::chapter_layout::types::Images,
    css: newt_core::css::CssStylesheetBuilder<'a>,
    text: &[u8],
) -> Result<bool, newt_core::chapter_layout::types::Capacity> {
    let mut engine = newt_core::chapter_layout::engine::Engine::new(
        params, storage, sink, breaker, images, None, css,
    )?;
    engine.start(b"body", None, None, None, None, None);
    engine.start(b"p", None, None, None, None, None);
    engine.text(text);
    engine.end(b"p");
    engine.end(b"body");
    Ok(engine.finish())
}
