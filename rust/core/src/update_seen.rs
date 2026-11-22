// SPDX-License-Identifier: MIT
//! Seen-build NWTS durable record wire codec.
//! The existing identity/store owners and first-paint confirmation stay outside this leaf.
#![forbid(unsafe_code)]
use crate::durable::Codec;
use crate::wire::{fnv1a, get32, put32};
pub use crate::update_card::slot_installer::identity::NewtAppIdentity;

pub const BYTES: usize = 56;
pub const MAGIC: u32 = 0x5354574e;
pub const VERSION: u32 = 1;
pub const NAMESPACE: &str = "newt_seen";

pub struct SeenBuildCodec;
impl SeenBuildCodec {
    /// None is the original null boundary. size is the declared readable extent;
    /// a larger backing may lend its prefix, but unreadable extents are refused.
    pub fn known(bytes: Option<&[u8]>, size: usize) -> bool {
        bytes.is_some_and(|bytes| size == BYTES && bytes.len() >= size && get32(bytes, 0) == MAGIC && get32(bytes, 4) == VERSION)
    }
    pub fn decode_into(bytes: Option<&[u8]>, size: usize, value: &mut NewtAppIdentity) -> bool {
        if !Self::known(bytes, size) { return false; }
        let Some(bytes) = bytes else { return false };
        if get32(bytes, 52) != fnv1a(&bytes[..52]) { return false; }
        let mut decoded = NewtAppIdentity::default();
        decoded.version.copy_from_slice(&bytes[12..36]);
        decoded.build.copy_from_slice(&bytes[36..52]);
        if decoded.version[0] == 0 || decoded.version[23] != 0 || decoded.build[15] != 0 { return false; }
        *value = decoded;
        true
    }
    pub fn encode(value: &NewtAppIdentity, sequence: u32, bytes: &mut [u8; BYTES]) -> bool {
        if value.version[0] == 0 || value.version[23] != 0 || value.build[15] != 0 { return false; }
        bytes.fill(0);
        put32(bytes, 0, MAGIC);put32(bytes, 4, VERSION);put32(bytes, 8, sequence);
        bytes[12..36].copy_from_slice(&value.version);
        bytes[36..52].copy_from_slice(&value.build);
        let checksum = fnv1a(&bytes[..52]);put32(bytes, 52, checksum);
        true
    }
}
impl Codec<BYTES> for SeenBuildCodec {
    type Value = NewtAppIdentity;
    const NAMESPACE: &'static str = crate::update_seen::NAMESPACE;
    const REPLACE_DAMAGED: bool = true;
    fn known(bytes: &[u8]) -> bool { Self::known(Some(bytes), bytes.len()) }
    fn decode(bytes: &[u8]) -> Option<NewtAppIdentity> {
        let mut value = NewtAppIdentity::default();
        Self::decode_into(Some(bytes), bytes.len(), &mut value).then_some(value)
    }
    fn encode(value: NewtAppIdentity, sequence: u32, bytes: &mut [u8; BYTES]) -> bool { Self::encode(&value, sequence, bytes) }
}
