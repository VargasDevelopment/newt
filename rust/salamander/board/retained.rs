// SPDX-License-Identifier: MIT
//! Short acquisition of the two native RTC regions; record policy stays Rust.
use super::{Board, Error, retained_words::RetainedWords, status};

#[repr(C)]
#[derive(Default)]
struct Regions {
    sleep: *mut u32,
    sleep_words: usize,
    sd_resume: *mut u32,
    sd_resume_words: usize,
}
const _: () = {
    assert!(core::mem::size_of::<Regions>() == 4 * core::mem::size_of::<usize>());
    assert!(core::mem::offset_of!(Regions, sd_resume) == 2 * core::mem::size_of::<usize>());
};

// SAFETY: the Y native port lends only the image's existing 68/140-byte RTC
// objects. It initializes all output fields synchronously, without a callback.
unsafe extern "C" {
    fn nb_retained_words(out: *mut Regions) -> i32;
}

#[derive(Debug, PartialEq, Eq)]
pub enum RetainedError {
    Native(Error),
    Region(super::retained_words::Error),
}

impl Board<'_> {
    /// Nothing in the returned value can retain this epoch's RTC capability.
    /// The sole Board acquisition excludes native/task/interrupt RTC aliases.
    pub fn with_retained<R>(
        &mut self,
        operation: impl for<'rtc> FnOnce(RetainedWords<'rtc>) -> R,
    ) -> Result<R, RetainedError> {
        // SAFETY: Board's Y acquisition includes the exclusive retained owners.
        unsafe { with_retained(operation) }
    }
}

/// Also used before panel/input startup, at frozen's boot-snapshot point.
///
/// # Safety
/// Link the native retained port, with its sole initialized, immovable
/// 68/140-byte RTC owners. No other task, interrupt or native access or loan may
/// coexist. Ownership may pass to Board only after this callback returns.
/// All cold words are initialized arbitrary values, never enum/bool objects.
pub unsafe fn with_retained<R>(
    operation: impl for<'rtc> FnOnce(RetainedWords<'rtc>) -> R,
) -> Result<R, RetainedError> {
    let mut regions = Regions::default();
    // SAFETY: exclusive initialized output; native retains no output pointer.
    status(unsafe { nb_retained_words(&mut regions) }).map_err(RetainedError::Native)?;
    // SAFETY: the caller grants both entire native RTC owners exclusively for
    // this short epoch. The callback result cannot retain either region.
    let words = unsafe {
        RetainedWords::from_raw_regions(regions.sleep, regions.sleep_words,
                                       regions.sd_resume, regions.sd_resume_words)
    }.map_err(RetainedError::Region)?;
    Ok(operation(words))
}
