// SPDX-License-Identifier: MIT
//! Streaming GitHub release JSON parser for online firmware checks.
//! Transient caller-owned check state only; no HTTP, document buffer or owner.
#![forbid(unsafe_code)]

#[path = "update_release/parser.rs"]
mod parser;
pub use parser::ReleaseJson;

pub const ASSET_URL_BYTES: usize = 512;

pub struct ReleaseAssets {
    pub manifest: [u8; ASSET_URL_BYTES],
    pub image: [u8; ASSET_URL_BYTES],
    pub image_size: i64,
    pub repeated: bool,
}
impl Default for ReleaseAssets {
    fn default() -> Self {
        Self { manifest: [0; ASSET_URL_BYTES], image: [0; ASSET_URL_BYTES], image_size: -1, repeated: false }
    }
}
