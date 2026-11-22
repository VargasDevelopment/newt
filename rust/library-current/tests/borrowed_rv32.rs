#![no_std]
//! Target-probe body: attach to the owner's existing RV32 Run only.
//! Catalog and native adapter are caller-owned; no table materialization here.
use newt_library_current::{Catalog, list::Files};
#[inline(never)]
pub fn borrowed_library_body(
    catalog: &mut Catalog,
    files: &mut dyn Files,
    index: usize,
    output: &mut [u8; 256],
) -> (usize, bool, bool) {
    let count = catalog.size();
    let readable = catalog.readable(files, index);
    let path = catalog.path(files, index, output);
    (count, readable, path)
}
