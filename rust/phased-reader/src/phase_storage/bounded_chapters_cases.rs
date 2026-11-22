// SPDX-License-Identifier: MIT
//! SDK's optional stylesheet attempt precedes its mandatory page arena.
//! These are allocator-policy regressions, not C++ ABI or frame evidence.
extern crate std;
use super::*;
use std::boxed::Box;

#[repr(align(16))]
struct Bank([u8; LAYOUT_BYTES]);

fn boundaries(bank: &mut [u8]) -> (usize, usize, usize) {
    let base = bank.as_ptr() as usize;
    let mut parts = Parts(bank);
    let Layout { storage, mut chapter_css } = storage(&mut parts).unwrap();
    assert_eq!(storage.page.as_ptr() as usize % 16, 0);
    chapter_css.add_text(b"p { font-weight: bold; }");
    assert_eq!(chapter_css.finish().rule_count(), 1);
    let rules_end = chapter_css.finish().rules.as_ptr() as usize - base
        + MAX_RULES * core::mem::size_of::<CssRule>();
    (storage.style_text.as_ptr() as usize - base + storage.style_text.len(),
        rules_end, storage.page.len())
}

#[test]
fn denied_optional_rules_stay_inert_and_leave_space_for_the_page() {
    let mut bank = Box::new(Bank([0; LAYOUT_BYTES]));
    let (style_end, rules_end, page_bytes) = boundaries(&mut bank.0);
    let capacity = style_end + 15 + page_bytes;
    assert!(capacity < rules_end);
    let mut parts = Parts(&mut bank.0[..capacity]);
    let Layout { storage, mut chapter_css } = storage(&mut parts).unwrap();
    assert_eq!(storage.style_text.len(), 3072);
    assert_eq!(storage.page.len(), page_bytes);
    assert_eq!(storage.page.as_ptr() as usize % 16, 0);
    chapter_css.add_text(b"p { font-weight: bold; }");
    assert_eq!(chapter_css.finish().rule_count(), 0);
}

#[test]
fn successful_optional_rules_can_exhaust_the_later_mandatory_page() {
    let mut bank = Box::new(Bank([0; LAYOUT_BYTES]));
    let (_, rules_end, _) = boundaries(&mut bank.0);
    let mut parts = Parts(&mut bank.0[..rules_end]);
    assert!(matches!(storage(&mut parts), Err(Status::OutOfMemory)));
    // The stylesheet really occupied this bank; the failed page request did
    // not rewind it, silently discard the rules, or allocate another bank.
    assert_eq!(parts.0.len(), 0);
}
