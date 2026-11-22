// SPDX-License-Identifier: MIT
//! Short loans of the existing initialized RTC words, without record policy.
#![deny(unsafe_op_in_unsafe_fn)]

use core::{marker::PhantomData, mem::{align_of, size_of}};

pub const SLEEP_WORDS: usize = 17;
pub const SD_RESUME_WORDS: usize = 35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Sleep,
    SdResume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Length(Region),
    Null(Region),
    Alignment(Region),
    SpanOverflow(Region),
    Overlap,
}

fn span(
    pointer: *mut u32,
    words: usize,
    expected: usize,
    region: Region,
) -> Result<(usize, usize), Error> {
    if words != expected {
        return Err(Error::Length(region));
    }
    if pointer.is_null() {
        return Err(Error::Null(region));
    }
    let start = pointer.addr();
    if start % align_of::<u32>() != 0 {
        return Err(Error::Alignment(region));
    }
    let end = start.checked_add(expected * size_of::<u32>())
        .ok_or(Error::SpanOverflow(region))?;
    Ok((start, end))
}

/// Two exclusive borrowed regions; no owned words, codec, snapshot or clearing policy.
/// Kept on the acquiring owner thread, like Board; the marker has no backing storage.
pub struct RetainedWords<'rtc> {
    sleep: &'rtc mut [u32; SLEEP_WORDS],
    sd_resume: &'rtc mut [u32; SD_RESUME_WORDS],
    _local: PhantomData<*mut ()>,
}

impl<'rtc> RetainedWords<'rtc> {
    /// Check both complete spans before creating either reference.
    ///
    /// # Safety
    /// If the structural checks succeed, each pointer must have provenance for
    /// its entire live, writable, initialized region, immovable for `'rtc`.
    /// Every word must be initialized, even when cold RTC contents are arbitrary.
    /// No other Rust/native/task/interrupt/hardware access may use either region
    /// for `'rtc`; the caller grants the unique loan, including across callbacks.
    /// These are ordinary retained memory regions, not MMIO. Address checks do
    /// not establish allocation validity, initialization or exclusive ownership.
    /// Arguments rejected by the structural checks are never dereferenced.
    pub unsafe fn from_raw_regions(
        sleep: *mut u32,
        sleep_words: usize,
        sd_resume: *mut u32,
        sd_resume_words: usize,
    ) -> Result<Self, Error> {
        let a = span(sleep, sleep_words, SLEEP_WORDS, Region::Sleep)?;
        let b = span(sd_resume, sd_resume_words, SD_RESUME_WORDS, Region::SdResume)?;
        if a.0 < b.1 && b.0 < a.1 {
            return Err(Error::Overlap);
        }
        // SAFETY: the caller grants initialized, exclusive regions for 'rtc.
        // Both exact lengths, non-null/aligned starts, non-wrapping ends and
        // disjoint byte spans were checked before either reference is formed.
        Ok(unsafe {
            Self {
                sleep: &mut *sleep.cast::<[u32; SLEEP_WORDS]>(),
                sd_resume: &mut *sd_resume.cast::<[u32; SD_RESUME_WORDS]>(),
                _local: PhantomData,
            }
        })
    }

    pub fn read_sleep<R>(
        &self,
        read: impl for<'loan> FnOnce(&'loan [u32; SLEEP_WORDS]) -> R,
    ) -> R {
        read(&*self.sleep)
    }

    pub fn write_sleep<R>(
        &mut self,
        write: impl for<'loan> FnOnce(&'loan mut [u32; SLEEP_WORDS]) -> R,
    ) -> R {
        write(&mut *self.sleep)
    }

    pub fn read_sd_resume<R>(
        &self,
        read: impl for<'loan> FnOnce(&'loan [u32; SD_RESUME_WORDS]) -> R,
    ) -> R {
        read(&*self.sd_resume)
    }

    pub fn write_sd_resume<R>(
        &mut self,
        write: impl for<'loan> FnOnce(&'loan mut [u32; SD_RESUME_WORDS]) -> R,
    ) -> R {
        write(&mut *self.sd_resume)
    }

    /// The caller decides when the existing first-word magic is consumed.
    pub fn clear_sleep_magic(&mut self) {
        self.sleep[0] = 0;
    }

    /// Independent of SleepRecord; all other SD resume words stay untouched.
    pub fn clear_sd_resume_magic(&mut self) {
        self.sd_resume[0] = 0;
    }
}
