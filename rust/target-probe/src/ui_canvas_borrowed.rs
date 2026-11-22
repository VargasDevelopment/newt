//! Standalone target-probe seam; not part of the safe library or host binary.
use newt_ui_canvas as ui;
#[repr(C)]
pub struct Input {
    pub x: i32,
    pub top: i32,
    pub pixels: u32,
    pub width: i32,
    pub bottom: i32,
    pub flags: u32,
}
/// # Safety
/// Input, text[len], frame[52272], out[capacity], results[4] are initialized,
/// aligned where typed, disjoint, valid for the call, exclusively owned where
/// mutable, and not concurrently accessed. Caller supplies bounded integer
/// geometry (all intermediate signed expressions fit). No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ui_canvas_borrowed(
    input: *const Input,
    text: *const u8,
    len: usize,
    frame: *mut u8,
    out: *mut u8,
    capacity: usize,
    results: *mut i32,
) -> i32 {
    if input.is_null()
        || text.is_null()
        || frame.is_null()
        || out.is_null()
        || results.is_null()
        || len > 8192
        || capacity > 4096
    {
        return -1;
    }
    let r = unsafe { &*input };
    let text = unsafe { core::slice::from_raw_parts(text, len) };
    let frame = unsafe { &mut *frame.cast::<ui::Frame>() };
    let out = unsafe { core::slice::from_raw_parts_mut(out, capacity) };
    let results = unsafe { core::slice::from_raw_parts_mut(results, 4) };
    results[0] = ui::fold_text(Some(text), out) as i32;
    results[1] = ui::text_width(Some(text), r.pixels, r.flags & 1 != 0);
    let mut canvas = ui::UiCanvas::portrait(Some(frame));
    results[2] =
        canvas.fixed_complete_paragraph(r.x, r.top, Some(text), r.pixels, r.width, r.bottom);
    // Target excludes the host-only paragraph counter.
    results[3] = 0;
    // Consume caller frame after production drawing, never a fixed sample.
    let checksum = frame
        .iter()
        .fold(0u32, |a, b| a.rotate_left(1) ^ u32::from(*b));
    checksum as i32
}
