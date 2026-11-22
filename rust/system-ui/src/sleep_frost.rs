// SPDX-License-Identifier: MIT
//! Frozen sleep_scene.cpp frost, retaining overwritten rows in lent scratch.
use crate::{Frame, Geometry, HomePlane};
use newt_core::frame;

const RADIUS: usize = 12;
const ROWS: usize = RADIUS + 1;
// A landscape row is the longest: frame::HEIGHT pixels.
pub const SLEEP_FROST_SCRATCH_BYTES: usize = frame::HEIGHT + ROWS * (frame::HEIGHT / 8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepFrostError {
    Geometry,
    Scratch,
}

fn bit(row: &[u8], x: usize) -> u8 {
    (row[x / 8] >> (7 - (x & 7))) & 1
}

/// Rebuild the full raw scene before each plane. The ring preserves only
/// source rows already overwritten; incoming rows remain raw in the frame.
pub fn frost_sleep_backdrop_in_place(
    frame: &mut Frame,
    scratch: &mut [u8],
    geometry: Geometry,
    plane: HomePlane,
) -> Result<(), SleepFrostError> {
    let (width, height) = (usize::from(geometry.width), usize::from(geometry.height));
    if !frame::is_frame_size(width, height) {
        return Err(SleepFrostError::Geometry);
    }
    let stride = width / 8;
    let bytes = width + ROWS * stride;
    let scratch = scratch.get_mut(..bytes).ok_or(SleepFrostError::Scratch)?;
    let (columns, history) = scratch.split_at_mut(width);
    columns.fill(0);
    for row in 0..=RADIUS {
        let source = &frame[row * stride..(row + 1) * stride];
        for (x, column) in columns.iter_mut().enumerate() {
            *column += bit(source, x);
        }
    }
    const BAYER: [u32; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
    for y in 0..height {
        if y > 0 {
            if y > RADIUS {
                let row = (y - RADIUS - 1) % ROWS;
                let source = &history[row * stride..(row + 1) * stride];
                for (x, column) in columns.iter_mut().enumerate() {
                    *column -= bit(source, x);
                }
            }
            if y + RADIUS < height {
                let row = y + RADIUS;
                let source = &frame[row * stride..(row + 1) * stride];
                for (x, column) in columns.iter_mut().enumerate() {
                    *column += bit(source, x);
                }
            }
        }
        let row = y % ROWS;
        history[row * stride..(row + 1) * stride]
            .copy_from_slice(&frame[y * stride..(y + 1) * stride]);
        let rows = ((height - 1).min(y + RADIUS) - y.saturating_sub(RADIUS) + 1) as u32;
        let mut black: u32 = columns[..=RADIUS].iter().map(|v| u32::from(*v)).sum();
        for x in 0..width {
            if x > 0 {
                if x > RADIUS {
                    black -= u32::from(columns[x - RADIUS - 1]);
                }
                if x + RADIUS < width {
                    black += u32::from(columns[x + RADIUS]);
                }
            }
            let area = rows * ((width - 1).min(x + RADIUS) - x.saturating_sub(RADIUS) + 1) as u32;
            let denominator = 255 * area;
            let light = (denominator - black * 128) * 3;
            let threshold = BAYER[(y & 3) * 4 + (x & 3)];
            let mut tone = light / denominator;
            if tone < 3 && (light % denominator) * 16 > denominator * threshold {
                tone += 1;
            }
            let set = match plane {
                HomePlane::Monochrome => black * 128 * 16 > denominator * threshold,
                HomePlane::Lsb => tone & 1 != 0,
                HomePlane::Msb => tone & 2 != 0,
            };
            let at = y * width + x;
            let mask = 0x80 >> (at & 7);
            if set {
                frame[at / 8] |= mask;
            } else {
                frame[at / 8] &= !mask;
            }
        }
    }
    Ok(())
}
