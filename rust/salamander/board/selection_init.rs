// SPDX-License-Identifier: MIT
//! Field-wise construction directly in the existing selection allocation.
use core::{mem::{MaybeUninit, size_of}, ptr::{addr_of_mut, write_bytes}};
use newt_phased_reader::{Selection, selection_initialization::{
    HREF, TITLE, MAX_TARGETS, STRING_BYTES, Navigation, navigation as nav,
}, zip_container::Entry};

/// No Selection, Navigation, or large array is ever constructed on the stack.
#[inline(never)]
pub fn initialize(output: &mut MaybeUninit<Selection>) {
    let slot = output.as_mut_ptr();
    // SAFETY: output is an exclusive, aligned slot, and no typed Selection is
    // borrowed until every field below is initialized. The private crates
    // supply compiler-derived offsets and exhaustive field-list assertions.
    // The small default wrappers are repr(transparent) over their private
    // field types, so each write establishes that field's actual Rust value.
    unsafe {
        addr_of_mut!((*slot).entry).write(Entry::default());
        write_bytes(slot.cast::<u8>().add(HREF.0), 0, HREF.1);
        write_bytes(slot.cast::<u8>().add(TITLE.0), 0, TITLE.1);
        addr_of_mut!((*slot).spines).write(0);
        initialize_navigation(&mut *addr_of_mut!((*slot).navigation).cast::<MaybeUninit<Navigation>>());
    }
}

/// Both owners initialize the same navigation value in its final allocation.
pub(super) fn initialize_navigation(output: &mut MaybeUninit<Navigation>) {
    let navigation = output.as_mut_ptr().cast::<u8>();
    // SAFETY: the compiler-derived offsets name every private field; typed
    // defaults establish the enum values before any Navigation reference exists.
    unsafe {
        for index in 0..MAX_TARGETS {
            navigation.add(nav::TARGETS).cast::<nav::TargetValue>().add(index)
                .write(nav::TargetValue::default());
        }
        write_bytes(navigation.add(nav::STRINGS), 0, STRING_BYTES);
        navigation.add(nav::PROJECTION).cast::<nav::ProjectionValue>()
            .write(nav::ProjectionValue::default());
        navigation.add(nav::USED).cast::<u16>().write(nav::INITIAL_USED);
        navigation.add(nav::GENERIC_LABEL).cast::<u16>().write(0);
        navigation.add(nav::BIND_ISSUES).cast::<u16>().write(0);
        navigation.add(nav::COUNT).write(0);
    }
}

const _: () = assert!(!core::mem::needs_drop::<Selection>());
const _: () = assert!(size_of::<nav::TargetValue>() == 24);
