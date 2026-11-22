// SPDX-License-Identifier: MIT
use super::{UiCanvas, kSleepBrand, kSleepBrandMaskRuns, MASK_FIRST};

/// The existing approved masked brand, borrowed for one synchronous paint.
/// Asset ownership and pixel/selector policy stay shared with Sleep.
pub fn draw_brand_stamp(c: &mut UiCanvas<'_>, left: i32, top: i32, selector: bool) {
        let mut run = 0;
        let mut remaining = kSleepBrandMaskRuns[0];
        let mut opaque = MASK_FIRST;
        for y in 0..416 {
            for x in 0..304 {
                let index = (y * 304 + x) as usize;
                if remaining == 0 {
                    run += 1;
                    remaining = kSleepBrandMaskRuns[run];
                    opaque = !opaque;
                }
                remaining -= 1;
                if opaque {
                    let black = kSleepBrand[index / 8] & (0x80 >> (index & 7)) != 0;
                    c.pixel(left + x, top + y, black != selector);
                }
            }
        }
}
