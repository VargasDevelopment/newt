// SPDX-License-Identifier: MIT
//! Test-only protocol adapter; include as a module in rust/host/src/main.rs. MIT. See the repository LICENSE.
use newt_core::frame::{self, ActionFrameResult, Frame, NativeRegion as Region};
use newt_core::preferences::Orientation;

fn load(word: &str) -> Frame {
    super::hex(word).try_into().expect("frame size")
}

fn orientation(word: &str) -> Orientation {
    match word {
        "0" => Orientation::Portrait,
        "1" => Orientation::LandscapeCcw,
        _ => panic!("frame orientation"),
    }
}

fn regions(plan: usize, source: &Frame, native: &mut Frame) -> bool {
    // Positional construction keeps the oracle's uint16 coordinate tables easy
    // to compare. These are protocol fixtures, not production region policy.
    const fn r(x: u16, y: u16, width: u16, height: u16) -> Region {
        Region {
            x,
            y,
            width,
            height,
        }
    }
    match plan {
        0 => frame::to_native_selector_regions(source, native, &[r(0, 0, 792, 528)]),
        1 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(784, 527, 8, 1)]),
        2 => frame::to_native_selector_regions(
            source,
            native,
            &[r(568, 374, 8, 108), r(568, 210, 8, 108), r(568, 46, 8, 108)],
        ),
        3 => frame::to_native_selector_regions(source, native, &[r(8, 7, 24, 3), r(16, 8, 8, 1)]),
        4 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(8, 1, 0, 1)]),
        5 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(8, 1, 8, 0)]),
        6 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(1, 1, 8, 1)]),
        7 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(8, 1, 7, 1)]),
        8 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(792, 1, 8, 1)]),
        9 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(8, 528, 8, 1)]),
        10 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(784, 1, 16, 1)]),
        11 => frame::to_native_selector_regions(source, native, &[r(0, 0, 8, 1), r(8, 527, 8, 2)]),
        12 => frame::to_native_selector_regions(
            source,
            native,
            &[r(0, 0, 8, 1), r(65528, 65535, 65528, 65535)],
        ),
        13 => frame::to_native_selector_regions(
            source,
            native,
            &[r(0, 527, 792, 1), r(784, 0, 8, 528)],
        ),
        _ => panic!("frame plan"),
    }
}

fn patch(bytes: &mut Frame, fields: &[&str]) {
    let begin = super::number(fields[2]) as usize;
    let count = super::number(fields[3]) as usize;
    let mask = u8::try_from(super::number(fields[4])).expect("patch mask");
    assert!(begin <= frame::FRAME_BYTES && count <= frame::FRAME_BYTES - begin);
    for byte in &mut bytes[begin..begin + count] {
        *byte ^= mask;
    }
}

pub(super) fn command(fields: &[&str]) -> String {
    if matches!(fields[0], "frame_publish" | "frame_action") {
        let action = fields[0] == "frame_action";
        let mut published = load(fields[if action { 7 } else { 5 }]);
        let mut staging = load(fields[if action { 8 } else { 6 }]);
        let ready = if action {
            let count = super::number(fields[5]) as usize;
            let value = u8::try_from(super::number(fields[6])).expect("action value");
            assert!(count <= frame::FRAME_BYTES);
            frame::publish_with_action_storage(&mut published, &mut staging, |pixels, action| {
                action[..count].fill(value);
                patch(pixels, fields);
                fields[1] == "1"
            }) == ActionFrameResult::Published
        } else {
            frame::publish_staged_frame(&mut published, &mut staging, |pixels| {
                patch(pixels, fields);
                fields[1] == "1"
            })
        };
        return format!(
            "{} {} {}",
            u8::from(ready),
            super::encoded(&published),
            super::encoded(&staging)
        );
    }
    if fields[0] == "frame_rect" {
        let mut native = load(fields[5]);
        let ready = frame::fill_native_rect(
            &mut native,
            fields[1].parse().expect("left"),
            fields[2].parse().expect("top"),
            fields[3].parse().expect("width"),
            fields[4].parse().expect("height"),
        );
        return format!("{} {}", u8::from(ready), super::encoded(&native));
    }
    let prefix = fields[0] == "frame_prefix";
    let source = load(fields[if prefix { 3 } else { 2 }]);
    let mut native = load(fields[if prefix { 4 } else { 3 }]);
    let ready = match fields[0] {
        "frame_bw" => {
            frame::to_native_frame(&source, &mut native, orientation(fields[1]));
            true
        }
        "frame_selectors" => {
            frame::to_native_selectors(&source, &mut native, orientation(fields[1]));
            true
        }
        "frame_prefix" => frame::restore_native_frame_prefix(
            &source,
            &mut native,
            super::number(fields[2]) as usize,
            orientation(fields[1]),
        ),
        "frame_region" => regions(super::number(fields[1]) as usize, &source, &mut native),
        _ => panic!("frame command"),
    };
    format!("{} {}", u8::from(ready), super::encoded(&native))
}
