//! Runtime input seam only; unsafe ABI outside the safe production crate.
use newt_system_ui::*;
/// # Safety
/// `frame` and optional `backdrop` point to initialized, disjoint 52272-byte
/// buffers; mutable frame is exclusive for the call. `title` lends `len` bytes.
/// No pointers retained. No literal frame or full-frame local scratch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn system_ui_borrowed(
    frame: *mut Frame,
    backdrop: *const Frame,
    title: *const u8,
    len: usize,
    focus: u8,
    flags: u32,
) -> u32 {
    if frame.is_null() || title.is_null() || len > 8192 {
        return u32::MAX;
    }
    let frame = unsafe { &mut *frame };
    let backdrop = if backdrop.is_null() {
        None
    } else {
        Some(unsafe { &*backdrop })
    };
    let title = unsafe { core::slice::from_raw_parts(title, len) };
    let labels = [Some(title), Some(b"Second" as &[u8]), Some(b"Third")];
    let list = ListOverlay::new(Some(title), &labels).unwrap();
    if flags & 1 != 0 {
        render_list_overlay(&list, focus, backdrop, Some(frame));
    } else {
        let mut bar = SystemBar::default();
        bar.radio_on = flags & 2 != 0;
        let mut page = SystemPage::new(
            Some(frame),
            &bar,
            list_overlay_buttons(&list, focus),
            Some(title),
        );
        page.row(newt_system_ui::SYSTEM_BODY_TOP, focus.into(), Some(title), true);
    }
    frame
        .iter()
        .fold(0u32, |a, &b| a.rotate_left(1) ^ u32::from(b))
}
