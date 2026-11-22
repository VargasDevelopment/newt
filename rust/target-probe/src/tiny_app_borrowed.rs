//! Ordinary RV32 call bodies. Root must retain and exercise them with runtime loans.
use newt_core::app::{Action, AppState};
use newt_system_ui::SystemBar;
use newt_tiny_app::*;
use newt_ui_canvas::Frame;
#[inline(never)]
#[unsafe(no_mangle)]
pub fn tiny_codec(bytes: Option<&[u8]>, definition: &mut Definition) -> bool {
    decode(bytes, definition)
}
#[inline(never)]
#[unsafe(no_mangle)]
pub fn tiny_route(
    definition: &Definition,
    state: &mut State,
    core: &mut AppState,
    action: Action,
) -> bool {
    apply_intent(core, dispatch(definition, state, action))
}
#[inline(never)]
#[unsafe(no_mangle)]
pub fn tiny_paint(definition: &Definition, state: &State, bar: &SystemBar, frame: &mut Frame) {
    render(definition, state, bar, Some(frame));
}
