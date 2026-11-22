// SPDX-License-Identifier: MIT
//! The frozen fixture's stateless read view of immutable EPUB bytes.
use crate::zip_window::source::BookSource;
pub struct Source<'a>(pub &'a [u8]);
impl BookSource for Source<'_> {
    fn size(&self) -> u64 { self.0.len() as u64 }
    fn read_at(&mut self, at: u64, output: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else { return 0; };
        let Some(bytes) = self.0.get(at..) else { return 0; };
        let count = bytes.len().min(output.len()).min(i32::MAX as usize);
        output[..count].copy_from_slice(&bytes[..count]);
        count as i32
    }
}
