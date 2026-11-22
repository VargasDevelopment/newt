// SPDX-License-Identifier: MIT
//! Sealed Wi-Fi network NWTN wire codec. MIT. Opaque ciphertext; no Secret, cipher, RNG or storage owner.
#![forbid(unsafe_code)]
use crate::durable::Codec;
use crate::wire::{fnv1a, get32, put32};

pub const NAME_BYTES: usize = 32;
pub const PASSWORD_BYTES: usize = 64;
pub const NONCE_BYTES: usize = 12;
pub const PLAIN_BYTES: usize = 2 + NAME_BYTES + PASSWORD_BYTES;
pub const TAG_BYTES: usize = 16;
pub const SEALED: usize = 16;
pub const TAG: usize = SEALED + NONCE_BYTES + PLAIN_BYTES;
pub const BYTES: usize = TAG + TAG_BYTES + 4;
pub const MAGIC: u32 = 0x4e54574e;
pub const VERSION: u32 = 1;
pub const NAMESPACE: &str = "newt_net";
const _: () = assert!(BYTES <= 232);

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SealedWifiNetwork {
    pub network: bool,
    pub nonce: [u8; NONCE_BYTES],
    pub cipher: [u8; PLAIN_BYTES],
    pub tag: [u8; TAG_BYTES],
}
impl Default for SealedWifiNetwork {
    fn default() -> Self { Self { network: false, nonce: [0; NONCE_BYTES], cipher: [0; PLAIN_BYTES], tag: [0; TAG_BYTES] } }
}
pub struct WifiNetworkCodec;
impl WifiNetworkCodec {
    /// None is the original null boundary; size is the caller's readable count.
    /// A longer backing slice is allowed, but an unreadable declared extent is refused.
    pub fn known(bytes: Option<&[u8]>, size: usize) -> bool {
        bytes.is_some_and(|bytes| size == BYTES && bytes.len() >= size && get32(bytes, 0) == MAGIC && get32(bytes, 4) == VERSION)
    }
    /// Failed validation leaves every caller output field unchanged.
    pub fn decode_into(bytes: Option<&[u8]>, size: usize, value: &mut SealedWifiNetwork) -> bool {
        if !Self::known(bytes, size) { return false; }
        let Some(bytes) = bytes else { return false };
        if get32(bytes, BYTES - 4) != fnv1a(&bytes[..BYTES - 4]) { return false; }
        let kind = get32(bytes, 12);
        if kind > 1 { return false; }
        let mut decoded = SealedWifiNetwork { network: kind == 1, ..SealedWifiNetwork::default() };
        decoded.nonce.copy_from_slice(&bytes[SEALED..SEALED + NONCE_BYTES]);
        decoded.cipher.copy_from_slice(&bytes[SEALED + NONCE_BYTES..TAG]);
        decoded.tag.copy_from_slice(&bytes[TAG..TAG + TAG_BYTES]);
        if !decoded.network && decoded != SealedWifiNetwork::default() { return false; }
        *value = decoded;
        true
    }
    pub fn encode(value: &SealedWifiNetwork, sequence: u32, bytes: &mut [u8; BYTES]) -> bool {
        if !value.network && value != &SealedWifiNetwork::default() { return false; }
        bytes.fill(0);
        put32(bytes, 0, MAGIC);put32(bytes, 4, VERSION);put32(bytes, 8, sequence);put32(bytes, 12, u32::from(value.network));
        bytes[SEALED..SEALED + NONCE_BYTES].copy_from_slice(&value.nonce);
        bytes[SEALED + NONCE_BYTES..TAG].copy_from_slice(&value.cipher);
        bytes[TAG..TAG + TAG_BYTES].copy_from_slice(&value.tag);
        let checksum = fnv1a(&bytes[..BYTES - 4]);put32(bytes, BYTES - 4, checksum);
        true
    }
    /// Only magic/version/kind are authenticated; generation is stamped later.
    pub fn associated(network: bool) -> [u8; 12] {
        let mut header = [0; 12];
        put32(&mut header, 0, MAGIC);put32(&mut header, 4, VERSION);put32(&mut header, 8, u32::from(network));
        header
    }
}
impl Codec<BYTES> for WifiNetworkCodec {
    type Value = SealedWifiNetwork;
    const NAMESPACE: &'static str = crate::wifi_network_store::NAMESPACE;
    const REPLACE_DAMAGED: bool = true;
    fn known(bytes: &[u8]) -> bool { Self::known(Some(bytes), bytes.len()) }
    fn decode(bytes: &[u8]) -> Option<SealedWifiNetwork> {
        let mut value = SealedWifiNetwork::default();
        Self::decode_into(Some(bytes), bytes.len(), &mut value).then_some(value)
    }
    fn encode(value: SealedWifiNetwork, sequence: u32, bytes: &mut [u8; BYTES]) -> bool { Self::encode(&value, sequence, bytes) }
}

#[path = "wifi_network_store/workflow.rs"] pub mod workflow;
