use newt_core::app::*;
#[unsafe(no_mangle)]
pub fn newt_app_dispatch_borrowed(state: &mut AppState, action: Action) -> bool { dispatch(state,action) }
#[unsafe(no_mangle)]
pub fn newt_app_wake_borrowed(state: &mut AppState) -> u8 { wake(state);state.screen as u8 }
#[unsafe(no_mangle)]
pub fn newt_app_graph_borrowed(packed: &mut u8, action: Action) -> bool { step_stats_graph(packed,action) }
