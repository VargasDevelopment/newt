// SPDX-License-Identifier: MIT
//! SDK image-state operations, without first-paint or readback policy.
use super::{Board, Error};
use core::ffi::{c_char, c_void, CStr};
use newt_core::update_card::slot_installer::{identity::{OtaRecord, OTA_SECTOR_BYTES}, selection::OtaFlash};
#[path = "image/hash.rs"]
mod hash;
// SAFETY: these synchronous SDK calls retain no Rust pointers or owners.
unsafe extern "C" {
    fn nb_running_image_version() -> *const c_char;
    fn nb_running_image_digest(out: *mut u8);
    fn nb_running_image_state(out: *mut u32) -> i32;
    fn nb_mark_running_image_valid() -> i32;
    fn nb_image_app_partition(slot: u32) -> *const c_void;
    fn nb_image_ota_partition() -> *const c_void;
    fn nb_image_running_partition() -> *const c_void;
    fn nb_image_partition_bytes(partition: *const c_void, out: *mut u32) -> i32;
    fn nb_image_partition_read(partition: *const c_void, at: u32, out: *mut u8, bytes: usize) -> i32;
    fn nb_image_partition_erase(partition: *const c_void, at: u32, bytes: u32) -> i32;
    fn nb_image_partition_write(partition: *const c_void, at: u32, input: *const u8, bytes: usize) -> i32;
}
fn result(code: i32) -> Result<(), Error> {
    if code == 0 { Ok(()) } else { Err(Error::Native(code)) }
}
/// The running image's fixed-width, NUL-padded ESP-IDF version field.
pub fn running_image_version() -> &'static CStr {
    // SAFETY: the native port returns the immutable mapped descriptor field.
    // The image build requires its version to fit including the trailing NUL.
    unsafe { CStr::from_ptr(nb_running_image_version()) }
}
impl Board<'_> {
    /// The current firmware's SDK-provided ELF SHA-256, without hashing work.
    pub fn running_image_digest(&self, out: &mut [u8; 32]) {
        // SAFETY: the initialized SDK descriptor is immutable; the exclusive
        // 32-byte output lasts through this synchronous copy and is not retained.
        unsafe { nb_running_image_digest(out.as_mut_ptr()) }
    }
    pub fn running_image_state(&mut self) -> Result<u32, Error> {
        let mut state = 0;
        // SAFETY: initialized SDK and one live scalar output for this call.
        let result = unsafe { nb_running_image_state(&mut state) };
        if result == 0 { Ok(state) } else { Err(Error::Native(result)) }
    }
    pub fn mark_running_image_valid(&mut self) -> Result<(), Error> {
        // SAFETY: actual running image; only SDK marks it and no pointer escapes.
        let result = unsafe { nb_mark_running_image_valid() };
        if result == 0 { Ok(()) } else { Err(Error::Native(result)) }
    }

    /// One SHA-256 over caller bytes; the mbedTLS context exists only for
    /// this call, as in `ImagePartitions::with_hash`.
    pub fn with_sha256<R>(&mut self, work: impl FnOnce(&mut dyn newt_core::update_online::Hash) -> R)
        -> Result<R, Error> {
        hash::with(work)
    }

    /// One short physical partition lookup. `ota_first` preserves select's
    /// frozen lookup order; found/verify do not acquire an otadata descriptor.
    pub fn with_image_partitions<R>(&mut self, ota_first: bool,
        work: impl FnOnce(&mut ImagePartitions<'_, '_>) -> R,
    ) -> R {
        // SAFETY: initialized sole SDK owner; SDK descriptors are immutable,
        // remain SDK-owned, and cannot escape the private callback view.
        let (ota, app, running) = unsafe {
            let ota = if ota_first { nb_image_ota_partition() } else { core::ptr::null() };
            (ota, [nb_image_app_partition(0), nb_image_app_partition(1)], nb_image_running_partition())
        };
        work(&mut ImagePartitions { _board: self, app, running, ota })
    }
}

/// Borrowed SDK descriptors, never a retained firmware selection or identity.
pub struct ImagePartitions<'op, 'sdk> {
    _board: &'op mut Board<'sdk>,
    app: [*const c_void; 2],
    running: *const c_void,
    ota: *const c_void,
}
impl<'sdk> ImagePartitions<'_, 'sdk> {
    /// Progress may briefly use this same Board after a partition SDK call
    /// returns. The exclusive reborrow prevents another partition operation.
    pub fn board(&mut self) -> &mut Board<'sdk> { self._board }
    pub fn app_present(&self, slot: usize) -> bool {
        self.app.get(slot).is_some_and(|part| !part.is_null())
    }
    pub fn running_slot(&self) -> i32 {
        self.app.iter().position(|part| !part.is_null() && *part == self.running)
            .map_or(-1, |slot| slot as i32)
    }
    fn bytes(partition: *const c_void) -> Result<u32, Error> {
        let mut bytes = 0;
        // SAFETY: only private actual SDK descriptors reach this call; native
        // rejects null and writes this live scalar only on success.
        result(unsafe { nb_image_partition_bytes(partition, &mut bytes) })?;
        Ok(bytes)
    }
    pub fn app_bytes(&self, slot: usize) -> Result<u32, Error> {
        Self::bytes(*self.app.get(slot).ok_or(Error::Mode)?)
    }
    pub fn ota_bytes(&self) -> Result<u32, Error> { Self::bytes(self.ota) }
    fn read(partition: *const c_void, at: u32, out: &mut [u8]) -> Result<(), Error> {
        // SAFETY: SDK descriptor and exclusive output cover this synchronous
        // read only; errors may leave an output prefix, just as esp_partition.
        result(unsafe { nb_image_partition_read(partition, at, out.as_mut_ptr(), out.len()) })
    }
    pub fn read_app(&mut self, slot: usize, at: u64, out: &mut [u8]) -> Result<(), Error> {
        let at = u32::try_from(at).map_err(|_| Error::Length)?;
        Self::read(*self.app.get(slot).ok_or(Error::Mode)?, at, out)
    }
    pub fn erase_app(&mut self, slot: usize, at: u32, bytes: u32) -> Result<(), Error> {
        let partition = *self.app.get(slot).ok_or(Error::Mode)?;
        // SAFETY: the scoped SDK descriptor cannot escape; native validates
        // the synchronous sector range before erasing it.
        result(unsafe { nb_image_partition_erase(partition, at, bytes) })
    }
    pub fn write_app(&mut self, slot: usize, at: u32, bytes: &[u8]) -> Result<(), Error> {
        let partition = *self.app.get(slot).ok_or(Error::Mode)?;
        // SAFETY: this readable input lasts through the synchronous SDK write;
        // native validates the descriptor and extent and retains no bytes.
        result(unsafe { nb_image_partition_write(partition, at, bytes.as_ptr(), bytes.len()) })
    }
    /// The real mbedTLS context exists only while this verification runs.
    pub fn with_hash<R>(&mut self,
        work: impl FnOnce(&mut Self, &mut dyn newt_core::update_online::Hash) -> R,
    ) -> Result<R, Error> {
        hash::with(|hash| work(self, hash))
    }
}
impl OtaFlash for ImagePartitions<'_, '_> {
    fn read(&mut self, at: u32, record: &mut OtaRecord) -> bool {
        let mut bytes = [0; 32];
        // Preserve a failed SDK read's partial prefix and untouched suffix,
        // matching its direct write into the frozen native OtaRecord.
        record.encode(&mut bytes);
        let read = Self::read(self.ota, at, &mut bytes).is_ok();
        *record = OtaRecord::decode(&bytes);
        read
    }
    fn erase(&mut self, at: u32) -> bool {
        // SAFETY: only the scoped actual otadata descriptor is used. Core's
        // commit supplies the sector offset; native SDK decides the result.
        unsafe { nb_image_partition_erase(self.ota, at, OTA_SECTOR_BYTES) == 0 }
    }
    fn write(&mut self, at: u32, record: &OtaRecord) -> bool {
        let mut bytes = [0; 32];
        record.encode(&mut bytes);
        // SAFETY: exact encoded record extent lives until synchronous return.
        unsafe { nb_image_partition_write(self.ota, at, bytes.as_ptr(), bytes.len()) == 0 }
    }
}
