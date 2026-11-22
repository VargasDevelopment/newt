// SPDX-License-Identifier: MIT
//! Compiler-derived fields for construction in the sample's existing heap bank.
use super::{Reader, MAX_ENTRIES};
use core::mem::offset_of;
pub use newt_reader_navigation::Navigation;

pub const CHAPTER_CHARACTERS: usize = offset_of!(Reader, chapter_characters);
pub const BOOK_CHARACTERS: usize = offset_of!(Reader, book_characters);
pub const SETTINGS: usize = offset_of!(Reader, settings);
pub const INFO: usize = offset_of!(Reader, info);
pub const NAVIGATION: usize = offset_of!(Reader, navigation);
pub const ACTIVE: usize = offset_of!(Reader, active);
pub const OPENED: usize = offset_of!(Reader, opened);

// Field additions and type changes must update the owning initializer too.
const _: fn(&Reader) = |Reader { chapter_characters: _, book_characters: _,
    settings: _, info: _, navigation: _, active: _, opened: _ }| {};
const _: fn(&Reader) -> &[u32; MAX_ENTRIES] = |value| &value.chapter_characters;
const _: fn(&Reader) -> &u64 = |value| &value.book_characters;
const _: fn(&Reader) -> &newt_core::preferences::ViewSettings = |value| &value.settings;
const _: fn(&Reader) -> &crate::PageInfo = |value| &value.info;
const _: fn(&Reader) -> &[Navigation; 2] = |value| &value.navigation;
const _: fn(&Reader) -> &usize = |value| &value.active;
const _: fn(&Reader) -> &bool = |value| &value.opened;
