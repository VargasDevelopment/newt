#![deny(unsafe_op_in_unsafe_fn)]
// Symbol exports only: safe Rust ABI, no pointer dereferences or FFI ABI claims.
#[path = "zip_window_borrowed.rs"]
mod borrowed;
use newt_core::zip_window::arena::Arena;

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_zip_window_probe(input: &[u8], storage: &mut [u8], output: &mut [u8], at: u64) -> bool {
    borrowed::read_borrowed(input, storage, output, at)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_zip_arena_probe(storage: &mut [u8], input: &[u8], alignment: usize) -> usize {
    let mut arena = Arena::new(storage);
    let mark = arena.mark();
    let allocation = arena.alloc(input.len(), alignment);
    if let Some(range) = allocation {
        if let Some(bytes) = arena.view_mut(range) {
            bytes.copy_from_slice(input);
        }
    }
    let copied = arena.copy_nul(Some(input));
    let checksum = copied
        .and_then(|range| arena.view(range))
        .map_or(0, |bytes| {
            bytes
                .iter()
                .fold(0usize, |acc, byte| acc.wrapping_add(*byte as usize))
        });
    arena.release(mark);
    checksum ^ arena.used() ^ arena.high_water() ^ arena.failed_alloc_size()
}
