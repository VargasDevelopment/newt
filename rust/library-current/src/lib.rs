//! Every-book catalog, directory scan and file list for Books. MIT.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
#[cfg(test)]
extern crate self as newt_library_current;
#[cfg(test)]
extern crate std;
pub mod catalog;
pub use newt_core::library_name;
pub mod list;
pub mod scan;
pub use catalog::*;
pub use library_name::valid_library_book_name;
pub use newt_core::library_catalog::{
    library_base_name, library_book_path, library_name_file, listable,
};

/// Reserve the actual storage fallibly, then initialize bytes in place.
fn zeroed(bytes: usize) -> Option<alloc::vec::Vec<u8>> {
    let mut storage = alloc::vec::Vec::new();
    #[cfg(test)]
    let bytes = catalog::allocation_tests::request(bytes);
    storage.try_reserve_exact(bytes).ok()?;
    storage.resize(bytes, 0);
    Some(storage)
}
