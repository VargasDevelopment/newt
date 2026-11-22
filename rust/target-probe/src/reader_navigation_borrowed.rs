//! Link-only borrowed navigation probe. Unsafe is confined to this wrapper.
use core::ffi::c_void;
use newt_reader_navigation::{Navigation, copy_label, metadata::Metadata};

#[repr(C)]
pub struct Anchor {
    pub hash: u32,
    pub start: u32,
}

// nm measures this target-compiled symbol's extent, not a host or guessed size.
#[unsafe(no_mangle)]
pub static navigation_type_size: [u8; core::mem::size_of::<Navigation>()] =
    [0; core::mem::size_of::<Navigation>()];

/// # Safety
/// Every opaque pointer denotes an aligned, initialized live Rust value of its
/// stated type: state is Navigation, metadata is Metadata with valid immutable
/// arena/package/manifest/spine/TOC/ZIP borrows. The metadata and its backing
/// storage remain immutable for the call; the metadata has nonempty spine/TOC. State is exclusively borrowed and
/// disjoint from all metadata/backing allocations. No pointer is retained.
/// C does not construct Rust values or infer their ABI from the measured size.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn navigation_bind_runtime(
    state: *mut c_void,
    metadata: *const c_void,
    selected: u32,
    required: u16,
) -> u32 {
    if state.is_null() || metadata.is_null() {
        return u32::MAX;
    }
    let state = unsafe { &mut *state.cast::<Navigation>() };
    let metadata = unsafe { &*metadata.cast::<Metadata<'_, '_>>() };
    state.stage_required_toc(required);
    u32::from(state.bind(metadata, selected)) | (u32::from(state.issues()) << 16)
}

/// # Safety
/// State is an initialized, aligned, exclusive Navigation. Anchors points to
/// count initialized aligned Anchor values (count > 0); output has len initialized, writable
/// bytes (len > 0), for example a zero-filled buffer. All allocations are disjoint and valid through this call.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn navigation_anchors_runtime(
    state: *mut c_void,
    anchors: *const Anchor,
    count: usize,
    character: u32,
    output: *mut u8,
    len: usize,
) -> u32 {
    if state.is_null()
        || anchors.is_null()
        || output.is_null()
        || count == 0
        || count > 256
        || len == 0
        || len > 1024
    {
        return u32::MAX;
    }
    let state = unsafe { &mut *state.cast::<Navigation>() };
    let anchors = unsafe { core::slice::from_raw_parts(anchors, count) };
    let output = unsafe { core::slice::from_raw_parts_mut(output, len) };
    state.begin_anchors();
    for a in anchors {
        state.observe_anchor(a.hash, a.start);
    }
    let copied = copy_label(Some(state.section(character).label), output);
    u32::from(state.issues()) | (u32::from(copied) << 16)
}

/// # Safety
/// Navigation state (shared), menu (exclusive), and Metadata (shared) are
/// aligned initialized Rust values; Metadata's immutable backing borrows live
/// throughout the call. Mapping has count initialized writable u32s, count > 0.
/// Output has len initialized, writable bytes (for example zero-filled), len > 0. All these allocations, including
/// metadata backing storage, are disjoint. No concurrent access or retention.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn navigation_menu_runtime(
    state: *const c_void,
    menu: *mut c_void,
    metadata: *const c_void,
    mapping: *mut u32,
    count: usize,
    character: u32,
    focus: u32,
    output: *mut u8,
    len: usize,
) -> u32 {
    if state.is_null()
        || menu.is_null()
        || metadata.is_null()
        || mapping.is_null()
        || output.is_null()
        || count == 0
        || count > 128
        || len == 0
        || len > 1024
    {
        return u32::MAX;
    }
    let state = unsafe { &*state.cast::<Navigation>() };
    let menu = unsafe { &mut *menu.cast::<Navigation>() };
    let metadata = unsafe { &*metadata.cast::<Metadata<'_, '_>>() };
    let mapping = unsafe { core::slice::from_raw_parts_mut(mapping, count) };
    let output = unsafe { core::slice::from_raw_parts_mut(output, len) };
    let current = state.section(character);
    let bound = menu.bind_menu(metadata, current, focus, mapping);
    let info = menu.menu_info();
    let copied = copy_label(Some(menu.menu_row(info.focus).label), output);
    let reused = menu.reuse_menu(current, focus);
    info.count
        ^ info.focus.rotate_left(8)
        ^ (u32::from(bound) << 29)
        ^ (u32::from(copied) << 30)
        ^ (u32::from(reused) << 31)
}
