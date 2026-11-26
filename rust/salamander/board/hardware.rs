// SPDX-License-Identifier: MIT
//! Synchronous SDK observations on a caller-owned UI capability; no held facts.
use core::{marker::PhantomData, mem::{MaybeUninit, align_of, offset_of, size_of}};
use crate::Board;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HardwareError {
    Null,
    Unavailable,
    InvalidBoolean,
    Native(i32),
    Efuse(i32),
}

/// Raw SDK values, including values whose corresponding known flag is false.
/// Percentage plausibility and DeviceInfo publication belong to the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryStatus {
    pub percentage_known: bool,
    pub percentage: u16,
    pub millivolts_known: bool,
    pub millivolts: u16,
    pub charging_known: bool,
    pub charging: bool,
}

#[repr(C)]
struct RawBattery {
    percentage: u16,
    millivolts: u16,
    percentage_known: u8,
    millivolts_known: u8,
    charging_known: u8,
    charging: u8,
}

#[repr(C)]
struct RawFactoryMac {
    sdk_status: i32,
    bytes: [u8; 6],
}

const _: () = {
    assert!(size_of::<RawBattery>() == 8 && align_of::<RawBattery>() == 2);
    assert!(offset_of!(RawBattery, percentage) == 0);
    assert!(offset_of!(RawBattery, millivolts) == 2);
    assert!(offset_of!(RawBattery, percentage_known) == 4);
    assert!(offset_of!(RawBattery, millivolts_known) == 5);
    assert!(offset_of!(RawBattery, charging_known) == 6);
    assert!(offset_of!(RawBattery, charging) == 7);
    assert!(size_of::<RawFactoryMac>() == 12 && align_of::<RawFactoryMac>() == 4);
    assert!(offset_of!(RawFactoryMac, sdk_status) == 0);
    assert!(offset_of!(RawFactoryMac, bytes) == 4);
};

// SAFETY: private hardware.h ABI; initialized UI owner serializes Wire/ACTIVE
// and all calls are synchronous with private correctly sized/aligned outputs.
unsafe extern "C" {
    fn nb_hw_read_battery(out: *mut RawBattery) -> i32;
    fn nb_hw_free_heap(out: *mut u32) -> i32;
    fn nb_hw_min_free_heap(out: *mut u32) -> i32;
    fn nb_hw_largest_heap_block(out: *mut u32) -> i32;
    fn nb_hw_current_task_stack_low(out: *mut u32) -> i32;
    fn nb_hw_factory_mac(out: *mut RawFactoryMac) -> i32;
    fn nb_hw_random_word(out: *mut u32) -> i32;
}

fn status(code: i32) -> Result<(), HardwareError> {
    match code {
        0 => Ok(()),
        1 => Err(HardwareError::Null),
        2 => Err(HardwareError::Unavailable),
        other => Err(HardwareError::Native(other)),
    }
}

fn boolean(value: u8) -> Result<bool, HardwareError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HardwareError::InvalidBoolean),
    }
}

/// Caller-owned UI view of the one SDK battery owner, without a payload/pointer.
/// Not Clone/Copy or Send/Sync. Multiple derived UI views call synchronously;
/// none creates backing or permits callbacks, reentry or competing Wire access.
pub struct HardwareReader<'sdk>(PhantomData<(&'sdk mut (), *mut ())>);

impl<'sdk> Board<'sdk> {
    /// Derive SDK permission without retaining this per-turn Board borrow.
    /// Acquisition guarantees program-lifetime SDK ownership and excludes
    /// external/task/IRQ Wire users. Root retires views before committed power
    /// shutdown. No native operation, allocation, pointer or new state is made.
    pub fn hardware_reader(&self) -> HardwareReader<'sdk> {
        HardwareReader(PhantomData)
    }
}

impl HardwareReader<'_> {
    /// The paused UI task can read Wire without reentering its lent panel.
    pub fn clock_snapshot(&mut self) -> crate::ClockSnapshot { super::read_clock_snapshot() }

    /// One SDK readStatus under the acquired, sequential UI-task contract.
    pub fn battery_status(&mut self) -> Result<BatteryStatus, HardwareError> {
        let mut out = MaybeUninit::<RawBattery>::uninit();
        // SAFETY: the acquired capability contract excludes competing Wire/SDK access;
        // the private output is live, aligned and exactly the hardware.h layout.
        status(unsafe { nb_hw_read_battery(out.as_mut_ptr()) })?;
        // SAFETY: NB_HW_OK initializes all primitive fields, even unknown ones.
        let raw = unsafe { out.assume_init() };
        Ok(BatteryStatus {
            percentage_known: boolean(raw.percentage_known)?,
            percentage: raw.percentage,
            millivolts_known: boolean(raw.millivolts_known)?,
            millivolts: raw.millivolts,
            charging_known: boolean(raw.charging_known)?,
            charging: boolean(raw.charging)?,
        })
    }

    pub fn free_heap_bytes(&mut self) -> Result<u32, HardwareError> {
        let mut out = MaybeUninit::<u32>::uninit();
        // SAFETY: the acquired capability grants this call; one private u32 output.
        status(unsafe { nb_hw_free_heap(out.as_mut_ptr()) })?;
        // SAFETY: NB_HW_OK initializes the entire u32; zero remains a raw value.
        Ok(unsafe { out.assume_init() })
    }

    pub fn min_free_heap_bytes(&mut self) -> Result<u32, HardwareError> {
        let mut out = MaybeUninit::<u32>::uninit();
        // SAFETY: the acquired capability grants this call; one private u32 output.
        status(unsafe { nb_hw_min_free_heap(out.as_mut_ptr()) })?;
        // SAFETY: NB_HW_OK initializes the entire u32, in SDK bytes since boot.
        Ok(unsafe { out.assume_init() })
    }

    pub fn largest_heap_block_bytes(&mut self) -> Result<u32, HardwareError> {
        let mut out = MaybeUninit::<u32>::uninit();
        // SAFETY: the acquired capability grants this call; one private u32 output.
        status(unsafe { nb_hw_largest_heap_block(out.as_mut_ptr()) })?;
        // SAFETY: NB_HW_OK initializes the entire u32; no reserve/clamping policy.
        Ok(unsafe { out.assume_init() })
    }

    /// Pinned ESP-IDF reports calling-task minimum free stack in bytes.
    pub fn current_task_stack_low_bytes(&mut self) -> Result<u32, HardwareError> {
        let mut out = MaybeUninit::<u32>::uninit();
        // SAFETY: synchronous call on the owning UI task; no task handle is lent.
        status(unsafe { nb_hw_current_task_stack_low(out.as_mut_ptr()) })?;
        // SAFETY: NB_HW_OK initializes the entire raw SDK count; no conversion.
        Ok(unsafe { out.assume_init() })
    }

    /// Six raw C3 eFuse MAC bytes. Fingerprint formatting/hashing stays in DEV.
    pub fn factory_mac(&mut self) -> Result<[u8; 6], HardwareError> {
        let mut out = MaybeUninit::<RawFactoryMac>::uninit();
        // SAFETY: acquired SDK permission, with private exact hardware.h output.
        status(unsafe { nb_hw_factory_mac(out.as_mut_ptr()) })?;
        // SAFETY: NB_HW_OK initializes status and all bytes, even on SDK failure.
        let raw = unsafe { out.assume_init() };
        if raw.sdk_status != 0 {
            return Err(HardwareError::Efuse(raw.sdk_status));
        }
        Ok(raw.bytes)
    }

    /// One esp_random word, including zero; no extra entropy/freshness guarantee.
    pub fn random_word(&mut self) -> Result<u32, HardwareError> {
        let mut out = MaybeUninit::<u32>::uninit();
        // SAFETY: acquired SDK permission, synchronous call and private output.
        status(unsafe { nb_hw_random_word(out.as_mut_ptr()) })?;
        // SAFETY: NB_HW_OK initializes the raw SDK word; no nonzero fallback.
        Ok(unsafe { out.assume_init() })
    }
}

/// Opt-in diagnostic snapshot of (min_free_heap, largest_heap_block, current_task_stack_low).
#[cfg(feature = "page-turn-trace")]
pub fn sample_memory_lows() -> (u32, u32, u32) {
    let mut heap = 0u32;
    let mut largest = 0u32;
    let mut stack = 0u32;
    // SAFETY: synchronous read-only SDK heap/task-water-mark probes on the UI task;
    // each output is private, aligned, and initialized on NB_HW_OK.
    unsafe {
        if nb_hw_min_free_heap(&mut heap) != 0 { heap = 0; }
        if nb_hw_largest_heap_block(&mut largest) != 0 { largest = 0; }
        if nb_hw_current_task_stack_low(&mut stack) != 0 { stack = 0; }
    }
    (heap, largest, stack)
}

const _: () = assert!(size_of::<HardwareReader<'static>>() == 0);
