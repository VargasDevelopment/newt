//! Allocation-free chapter cache naming and segment layout.
#![forbid(unsafe_code)]
pub mod identity;
pub mod namespace;
pub mod segment;
/// Same 32-byte package digest shape, using Core's existing digest alias.
pub use crate::book_semantic_index::Digest;
/// C-string APIs consume bytes before the first NUL; length APIs do not.
pub(crate) fn cstr(raw: &[u8]) -> &[u8] {
    &raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())]
}

#[cfg(test)]
mod tests;
