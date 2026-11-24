// SPDX-License-Identifier: MIT
//! Rust-only checked boundaries, outside the valid C++ rectangle contract. MIT. See the repository LICENSE.
use newt_core::frame::{self, NativeRegion};
use newt_core::preferences::{self, Orientation, ViewSettings};

#[test]
fn frame_extent_matches_every_reader_orientation() {
    for orientation in Orientation::ALL {
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

fn pixel(frame: &frame::Frame, index: usize) -> bool {
    frame[index / 8] & (0x80 >> (index % 8)) != 0
}

/// Clockwise landscape is the counterclockwise picture turned half a turn:
/// native raster index i in CCW is index last-i in CW, polarity unchanged.
#[test]
fn clockwise_landscape_is_a_half_turn_of_counterclockwise() {
    let mut logical = [0; frame::FRAME_BYTES];
    let mut seed = 0x2545_f491_u32;
    for byte in &mut logical {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *byte = seed as u8;
    }
    let (mut ccw, mut cw) = ([0; frame::FRAME_BYTES], [0; frame::FRAME_BYTES]);
    let (mut ccw_gray, mut cw_gray) = ([0; frame::FRAME_BYTES], [0; frame::FRAME_BYTES]);
    frame::to_native_frame(&logical, &mut ccw, Orientation::LandscapeCcw);
    frame::to_native_frame(&logical, &mut cw, Orientation::LandscapeCw);
    frame::to_native_selectors(&logical, &mut ccw_gray, Orientation::LandscapeCcw);
    frame::to_native_selectors(&logical, &mut cw_gray, Orientation::LandscapeCw);
    let last = frame::FRAME_BYTES * 8 - 1;
    for at in 0..=last {
        assert_eq!(pixel(&cw, last - at), pixel(&ccw, at));
        assert_eq!(pixel(&cw_gray, last - at), pixel(&ccw_gray, at));
        assert_eq!(pixel(&ccw_gray, at), pixel(&logical, at), "selectors keep polarity");
    }
    for bytes in [0, 1, 99, frame::FRAME_BYTES] {
        let mut prefix = [0x69; frame::FRAME_BYTES];
        assert!(frame::restore_native_frame_prefix(&logical, &mut prefix, bytes, Orientation::LandscapeCw));
        assert_eq!(prefix[..bytes], cw[..bytes]);
        assert!(prefix[bytes..].iter().all(|&byte| byte == 0x69));
    }
}
