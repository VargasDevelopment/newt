// SPDX-License-Identifier: MIT
//! Update-boot secret fields and short key loans. Reading constructs none.
use core::{marker::{PhantomData, PhantomPinned}, ptr};
use newt_core::wifi_network_store::workflow::{KeyRegion, NativeWipe, ScopedSecrets, Secret};

/// Frozen secret.h volatile stores, including bytes whose lifetime ends here.
pub struct Wipe;
impl NativeWipe for Wipe {
    fn wipe(&self, bytes: &mut [u8]) {
        for byte in bytes {
            // SAFETY: each pointer denotes one live exclusively borrowed byte.
            // Volatile stores preserve the original non-elidable clearing.
            unsafe { ptr::write_volatile(byte, 0); }
        }
    }
}

/// Place empty fields in the final pinned UpdateView before writing secrets.
/// Live fields are edited/cleared in place: Rust moves cannot wipe their source.
/// No Clone, Copy, Default, formatting, pointer export or stored wipe callback.
#[repr(C)]
pub struct SecretBytes<const N: usize> {
    bytes: [u8; N],
    size: usize,
    pinned: PhantomPinned,
    ui_only: PhantomData<*mut ()>,
}
impl<const N: usize> SecretBytes<N> {
    pub const fn empty() -> Self {
        Self { bytes: [0; N], size: 0, pinned: PhantomPinned, ui_only: PhantomData }
    }
    pub fn mix(&mut self, fresh: &[u8; N]) {
        for (byte, fresh) in self.bytes.iter_mut().zip(fresh) { *byte ^= fresh; }
        self.size = N;
    }
}
impl<const N: usize> Secret<N> for SecretBytes<N> {
    fn size(&self) -> usize { self.size }
    fn bytes(&self) -> &[u8; N] { &self.bytes }
    fn clear(&mut self, wipe: &dyn NativeWipe) {
        wipe.wipe(&mut self.bytes);
        self.size = 0;
    }
    fn fill(&mut self, size: usize, wipe: &dyn NativeWipe,
        source: &mut dyn FnMut(&mut [u8]) -> bool,
    ) -> bool {
        self.clear(wipe);
        if size > N || !source(&mut self.bytes[..size]) {
            self.clear(wipe);
            return false;
        }
        self.size = size;
        true
    }
}
impl newt_device_ui::update::Password for SecretBytes<64> {
    fn append(&mut self, text: &[u8]) -> bool {
        let size = text.iter().position(|byte| *byte == 0).unwrap_or(text.len());
        if size == 0 || size > 64 - self.size { return false; }
        self.bytes[self.size..self.size + size].copy_from_slice(&text[..size]);
        self.size += size;
        true
    }
    fn erase(&mut self) -> bool {
        if self.size == 0 { return false; }
        while self.size > 1 && self.bytes[self.size - 1] & 0xc0 == 0x80 {
            self.size -= 1;
            self.bytes[self.size] = 0;
        }
        self.size -= 1;
        self.bytes[self.size] = 0;
        true
    }
}
impl<const N: usize> Drop for SecretBytes<N> {
    fn drop(&mut self) { self.clear(&Wipe); }
}

struct KeyLoan<'region, 'wipe> {
    region: KeyRegion<'region>,
    wipe: &'wipe dyn NativeWipe,
}
impl Secret<32> for KeyLoan<'_, '_> {
    fn size(&self) -> usize { *self.region.length }
    fn bytes(&self) -> &[u8; 32] { self.region.bytes }
    fn clear(&mut self, wipe: &dyn NativeWipe) {
        wipe.wipe(self.region.bytes);
        *self.region.length = 0;
    }
    fn fill(&mut self, size: usize, wipe: &dyn NativeWipe,
        source: &mut dyn FnMut(&mut [u8]) -> bool,
    ) -> bool {
        self.clear(wipe);
        if size > 32 || !source(&mut self.region.bytes[..size]) {
            self.clear(wipe);
            return false;
        }
        *self.region.length = size;
        true
    }
}
impl Drop for KeyLoan<'_, '_> {
    fn drop(&mut self) {
        self.wipe.wipe(self.region.bytes);
        *self.region.length = 0;
    }
}
pub struct Scopes;
impl ScopedSecrets for Scopes {
    fn with_key(&self, region: KeyRegion<'_>, wipe: &dyn NativeWipe,
        body: &mut dyn FnMut(&mut dyn Secret<32>),
    ) {
        let mut key = KeyLoan { region, wipe };
        key.clear(wipe);
        body(&mut key);
    }
}

#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(core::mem::size_of::<SecretBytes<64>>() == 68);
    assert!(core::mem::size_of::<SecretBytes<32>>() == 36);
};
