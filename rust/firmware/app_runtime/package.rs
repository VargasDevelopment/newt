// SPDX-License-Identifier: MIT
//! The installed app's boot load. newt_core::app_store holds the record rules;
//! this supplies the X3's SHA-256 and Tiny's decoder, the bundled Pocket Pause
//! and the frozen explanations. The two slot copies are fallible boot-only
//! heap scratch, never held beside the running app.
use newt_core::app_store::{self, Load, PACKAGE_BYTES, Package, Payload, RECORD_BYTES};
use newt_core::durable::Backend;
use newt_sdk_heap::Fixed;
use newt_tiny_app::{Definition, check, decode, pause::BUILTIN_BYTES};

const BUILTIN_DIGEST: [u8; 32] = [
    0xa0, 0xfc, 0x8e, 0xe6, 0x51, 0xcd, 0x3e, 0x50, 0xcf, 0x96, 0x49, 0xe1,
    0x6b, 0x01, 0x4e, 0xcf, 0x3c, 0xf7, 0x6b, 0x78, 0xe7, 0xa7, 0x1a, 0x43,
    0xcb, 0x66, 0xe1, 0x3e, 0xcc, 0xa5, 0x80, 0xab,
];
const UNREADABLE: &[u8] = b"The saved app could not be read. Please preserve its storage.";
const DAMAGED: &[u8] = b"The saved app did not pass its integrity check.";

/// Tiny's decoder alone interprets payloads.
struct X3Payload;
impl Payload for X3Payload {
    fn hash(&mut self, bytes: &[u8], digest: &mut [u8; 32]) -> bool {
        crate::reader_hash::package_hash(bytes, digest)
    }
    fn accepts(&mut self, wire: &[u8; PACKAGE_BYTES]) -> bool { check(wire) }
}

/// Confirmed absence alone selects the bundled Pocket Pause. Every error
/// leaves storage untouched and returns the frozen unavailable explanation.
pub fn load_boot<B: Backend>(store: &mut B, app: &mut Definition) -> Result<(), &'static [u8]> {
    let Some(mut slots) = Fixed::try_new_with(|| [[0; RECORD_BYTES]; 2]) else {
        return Err(UNREADABLE);
    };
    match app_store::load(store, &mut X3Payload, &mut slots) {
        // Already validated in this private store; no second SDK hash/read.
        Load::Selected(slot) => {
            if decode(Some(app_store::package(&slots[slot])), app) { Ok(()) } else { Err(DAMAGED) }
        }
        Load::Corrupt => Err(DAMAGED),
        Load::Empty => {
            let bundled = app_store::check_package(&mut X3Payload, BUILTIN_BYTES, &BUILTIN_DIGEST);
            if bundled == Package::Valid && decode(Some(BUILTIN_BYTES), app) { Ok(()) }
            else { Err(b"The bundled app did not pass its integrity check.") }
        }
        Load::Unsupported => Err(b"The saved app needs a different Newt package version."),
        Load::Ambiguous => Err(b"The saved app generations conflict. Recovery is needed."),
        Load::Failed => Err(UNREADABLE),
    }
}
