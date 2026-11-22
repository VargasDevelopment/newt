//! Borrowed atlas FFI for target code retention, never in the font/Core.
use newt_reader_atlas::{Geometry, ReaderAtlas, probe::{Request, reader_atlas_draw_runtime, reader_atlas_metrics_runtime}};
#[repr(C)]
pub struct Input { pub cp: u32, pub right: u32, pub px: u16, pub style: u8, pub reserved: u8, pub x: i32, pub baseline: i32 }
/// # Safety
/// C caller owns aligned initialized `Input[count]`, initialized writable
/// `frame[len]`, and aligned initialized writable `output[2]` for this call. These three allocations are disjoint
/// and valid for the complete call; no concurrent access or retained pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atlas_borrowed_runtime(inputs: *const Input, count: usize, frame: *mut u8, len: usize, output: *mut i64, landscape: u32) -> i32 {
    if inputs.is_null() || frame.is_null() || output.is_null() || count > 1024 || len != 52272 || landscape > 1 { return -1; }
    // Caller contract above supplies extent, alignment, exclusivity and lifetime.
    let inputs = unsafe { core::slice::from_raw_parts(inputs,count) };
    let frame = unsafe { core::slice::from_raw_parts_mut(frame,len) };
    let output = unsafe { core::slice::from_raw_parts_mut(output,2) };
    let mut font = ReaderAtlas;
    let geometry = Geometry::reader(landscape != 0);
    let mut metric = 0i64;
    let mut bitmap = 0i64;
    for r in inputs {
        let request = [Request { cp:r.cp, right:r.right, px:r.px, style:r.style, x:r.x, baseline:r.baseline }];
        metric = metric.wrapping_mul(33).wrapping_add(reader_atlas_metrics_runtime(&mut font,&request));
        bitmap ^= reader_atlas_draw_runtime(&font,&request,geometry,frame);
    }
    output[0] = metric;
    output[1] = bitmap;
    0
}
