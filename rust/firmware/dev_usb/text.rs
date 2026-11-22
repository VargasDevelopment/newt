// SPDX-License-Identifier: MIT
//! snprintf's bounded ASCII destination: reserve its NUL and keep truncation.
use core::fmt;

pub(super) struct Text<'a> {
    bytes: &'a mut [u8],
    used: usize,
    required: usize,
}
impl<'a> Text<'a> {
    pub(super) fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, used: 0, required: 0 }
    }
    pub(super) fn bytes(&mut self, value: &[u8]) {
        self.required = self.required.saturating_add(value.len());
        let count = value.len().min(self.bytes.len().saturating_sub(1 + self.used));
        self.bytes[self.used..self.used + count].copy_from_slice(&value[..count]);
        self.used += count;
    }
    pub(super) fn len(&self) -> usize { self.used }
    pub(super) fn required(&self) -> usize { self.required }
    pub(super) fn written(&self) -> &[u8] { &self.bytes[..self.used] }
}
impl fmt::Write for Text<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.bytes(value.as_bytes());
        Ok(())
    }
}
