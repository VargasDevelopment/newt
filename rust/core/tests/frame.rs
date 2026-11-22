// SPDX-License-Identifier: MIT
//! Rust-only checked boundaries, outside the valid C++ rectangle contract. MIT. See the repository LICENSE.
use newt_core::frame::{self, NativeRegion};
use newt_core::preferences::{self, Orientation, ViewSettings};

#[test]
fn frame_extent_matches_both_reader_orientations() {
    for orientation in [Orientation::Portrait, Orientation::LandscapeCcw] {
        let geometry = preferences::geometry(ViewSettings {
            orientation,
            ..ViewSettings::default()
        });
        assert_eq!(
            usize::from(geometry.width) * usize::from(geometry.height),
            frame::FRAME_BYTES * 8
        );
        let dimensions = (usize::from(geometry.width), usize::from(geometry.height));
        assert_eq!(
            dimensions,
            if orientation == Orientation::Portrait {
                (frame::WIDTH, frame::HEIGHT)
            } else {
                (frame::NATIVE_WIDTH, frame::NATIVE_HEIGHT)
            }
        );
    }
}

#[test]
fn empty_region_plan_preserves_every_byte() {
    let source = [0xff; frame::FRAME_BYTES];
    let mut native = [0x69; frame::FRAME_BYTES];
    let plan: [NativeRegion; 0] = [];
    assert!(!frame::to_native_selector_regions(
        &source,
        &mut native,
        &plan
    ));
    assert_eq!(native, [0x69; frame::FRAME_BYTES]);
}

#[test]
fn invalid_rectangles_fail_before_writing() {
    let mut native = [0x69; frame::FRAME_BYTES];
    for (left, top, width, height) in [
        (-1, 0, 1, 1),
        (0, -1, 1, 1),
        (0, 0, -1, 1),
        (0, 0, 1, -1),
        (528, 0, 1, 1),
        (0, 792, 1, 1),
        (527, 0, 2, 1),
        (0, 791, 1, 2),
        (529, 0, 0, 0),
        (0, 793, 0, 0),
        (i32::MAX, 0, 1, 1),
        (0, 0, i32::MAX, 1),
        (0, i32::MAX, 1, 1),
        (0, 0, 1, i32::MAX),
        (i32::MIN, 0, 0, 0),
    ] {
        assert!(!frame::fill_native_rect(
            &mut native,
            left,
            top,
            width,
            height
        ));
        assert_eq!(native, [0x69; frame::FRAME_BYTES]);
    }
}
