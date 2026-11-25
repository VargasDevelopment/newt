// SPDX-License-Identifier: MIT
//! Compiler-derived layout for the Board's field-wise initialization boundary.
use super::{BookLanguage, Selection};
use core::mem::offset_of;
use newt_core::member_check::Checked;
pub use newt_reader_navigation::Navigation;
pub use newt_reader_navigation::{MAX_TARGETS, STRING_BYTES, initialization as navigation};

pub const HREF: (usize, usize) = (offset_of!(Selection, href), 512);
pub const TITLE: (usize, usize) = (offset_of!(Selection, title), 256);

// Exhaustive patterns and field types make a storage-layout change fail here.
const _: fn(&Selection) = |Selection { entry: _, href: _, title: _, language: _, spines: _, navigation: _ }| {};
const _: fn(&Selection) -> &[u8; HREF.1] = |value| &value.href;
const _: fn(&Selection) -> &[u8; TITLE.1] = |value| &value.title;

impl Default for Selection {
    fn default() -> Self {
        Self {
            entry: Checked::default(),
            href: [0; 512],
            title: [0; 256],
            spines: 0,
            language: BookLanguage::default(),
            navigation: Navigation::default(),
        }
    }
}
