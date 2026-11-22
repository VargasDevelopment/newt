use crate::{
    CLOCK_TEXT_BYTES, Frame, Geometry, HEIGHT, HomePlane, SystemBar, SystemButtons, WIDTH, data,
    format_clock_text,
};
use core::ffi::CStr;
use newt_ui_canvas::atlas::{self, Face, Stamp};

fn cstr(t: Option<&[u8]>) -> &[u8] {
    let t = t.unwrap_or_default();
    &t[..t.iter().position(|&b| b == 0).unwrap_or(t.len())]
}
fn base_tone(x: i32, y: i32) -> u32 {
    let tile = (y / 8 * (WIDTH as i32 / 8) + x / 8) as usize;
    let byte = data::BASE_MAP[tile] as usize * 16 + (y % 8) as usize * 2 + (x % 8) as usize / 4;
    u32::from((data::BASE_TILES[byte] >> (6 - 2 * (x % 4))) & 3)
}
fn pixel(frame: &mut Frame, plane: HomePlane, x: i32, y: i32, tone: u32) {
    if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
        return;
    }
    let at = (y * WIDTH as i32 + x) as usize;
    let bit = 0x80 >> (at % 8);
    let set = match plane {
        HomePlane::Monochrome => tone < 2,
        HomePlane::Lsb => tone & 1 != 0,
        HomePlane::Msb => tone & 2 != 0,
    };
    if set {
        frame[at / 8] |= bit;
    } else {
        frame[at / 8] &= !bit;
    }
}
fn base_rows(frame: &mut Frame, plane: HomePlane, first: i32, end: i32, left: usize, right: usize) {
    // Packed selector gather; writes each requested byte exactly once.
    let shift = if plane == HomePlane::Lsb { 0 } else { 1 };
    let gather = |v: u8| ((v >> 3) & 8) | ((v >> 2) & 4) | ((v >> 1) & 2) | (v & 1);
    for y in first..end {
        for x in left..right {
            let source =
                data::BASE_MAP[y as usize / 8 * (WIDTH / 8) + x] as usize * 16 + y as usize % 8 * 2;
            let mut selectors = (gather(data::BASE_TILES[source] >> shift) << 4)
                | gather(data::BASE_TILES[source + 1] >> shift);
            if plane == HomePlane::Monochrome {
                selectors ^= 255;
            }
            frame[y as usize * (WIDTH / 8) + x] = selectors;
        }
    }
}
fn stamp(frame: &mut Frame, plane: HomePlane, s: Stamp, ox: i32, oy: i32, left: i32, right: i32) {
    atlas::tones(s, |position, value| {
        if value == 255 {
            return;
        }
        let x = ox + i32::from(s.x) + (position % u32::from(s.w)) as i32;
        let y = oy + i32::from(s.y) + (position / u32::from(s.w)) as i32;
        if x < left || y < 0 || x >= right || y >= HEIGHT as i32 {
            return;
        }
        let gray = base_tone(x, y) * 85 * value / 255;
        pixel(frame, plane, x, y, 3.min((gray + 42) / 85));
    });
}
fn fixed_width(text: &[u8], face: Face) -> i32 {
    data::LABELS
        .iter()
        .find(|l| l.face == face as u8 && l.text == text)
        .map_or_else(|| atlas::width64(Some(text), face), |l| l.advance)
}
fn home_text(
    frame: &mut Frame,
    plane: HomePlane,
    x: i32,
    baseline: i32,
    raw: Option<&[u8]>,
    face: Face,
    max_width: i32,
    anchor: u8,
) {
    // Exact HomeCanvas fit buffer, distinct from UiCanvas's 256-byte text buffer.
    let mut text = [0u8; 128];
    let raw = cstr(raw);
    let mut used = raw.len().min(127);
    text[..used].copy_from_slice(&raw[..used]);
    if fixed_width(&text[..used], face) > max_width * 64 {
        while used > 0 {
            used -= 1;
            while used > 0 && text[used] & 0xc0 == 0x80 {
                used -= 1;
            }
            text[used] = 0;
            if fixed_width(&text[..used], face) + fixed_width("…".as_bytes(), face)
                <= max_width * 64
                && used + 4 <= 128
            {
                text[used..used + 3].copy_from_slice("…".as_bytes());
                used += 3;
                break;
            }
        }
        if used == 0 {
            text[..3].copy_from_slice("…".as_bytes());
            used = 3;
        }
    }
    let text = &text[..used];
    let left = 0.max(
        x - if anchor == 2 {
            max_width
        } else if anchor == 1 {
            max_width / 2
        } else {
            0
        },
    );
    let right = (WIDTH as i32).min(left + max_width);
    if let Some(l) = data::LABELS
        .iter()
        .find(|l| l.face == face as u8 && l.anchor == anchor && l.text == text)
    {
        stamp(frame, plane, l.stamp, x, baseline, left, right);
        return;
    }
    let width = fixed_width(text, face);
    let mut pen = x * 64
        - if anchor == 1 {
            width / 2
        } else if anchor == 2 {
            width
        } else {
            0
        };
    let mut at = 0;
    let mut previous = 0;
    while at < text.len() {
        let cp = atlas::next(text, &mut at);
        pen += atlas::kern(face, previous, cp);
        // Signed division truncates toward zero, including negative pens.
        let snapped = ((pen + 4) / 8) * 8;
        stamp(
            frame,
            plane,
            atlas::glyph(face, cp, ((snapped % 64 + 64) % 64 / 8) as u8).stamp,
            snapped / 64,
            baseline,
            left,
            right,
        );
        pen += i32::from(atlas::glyph(face, cp, 0).advance);
        previous = cp;
    }
}
/// Status has the source's 48-byte scratch and exact owner-approved copy.
pub fn format_system_status(
    out: &mut [u8; 48],
    radio_on: bool,
    battery_known: bool,
    percent: u16,
) -> &[u8] {
    out.fill(0);
    let word = if radio_on {
        b"Wi-Fi" as &[u8]
    } else {
        b"Offline"
    };
    out[..word.len()].copy_from_slice(word);
    let mut at = word.len();
    let separator = " · ".as_bytes();
    out[at..at + separator.len()].copy_from_slice(separator);
    at += separator.len();
    if battery_known && percent <= 100 {
        if percent == 100 {
            out[at] = b'1';
            at += 1;
        }
        if percent >= 10 {
            out[at] = b'0' + ((percent / 10) % 10) as u8;
            at += 1;
        }
        out[at] = b'0' + (percent % 10) as u8;
        at += 1;
    } else {
        out[at..at + 2].copy_from_slice(b"--");
        at += 2;
    }
    out[at] = b'%';
    &out[..at + 1]
}
pub fn draw_frosted_system_header(bar: &SystemBar, plane: HomePlane, frame: Option<&mut Frame>) {
    let Some(frame) = frame else {
        return;
    };
    base_rows(frame, plane, 0, 64, 0, WIDTH / 8);
    let clock = CStr::from_bytes_until_nul(&bar.clock).unwrap_or(c"--:--");
    let mut formatted = [0; CLOCK_TEXT_BYTES];
    let clock = format_clock_text(Some(clock), bar.format, &mut formatted);
    home_text(
        frame,
        plane,
        264,
        31,
        clock.map(CStr::to_bytes),
        Face::R24,
        112,
        1,
    );
    let mut value = [0; 48];
    home_text(
        frame,
        plane,
        500,
        31,
        Some(format_system_status(
            &mut value,
            bar.radio_on,
            bar.battery_known,
            bar.battery_percent,
        )),
        Face::R21,
        180,
        2,
    );
}
pub fn draw_frosted_system_base(bar: &SystemBar, plane: HomePlane, frame: Option<&mut Frame>) {
    let Some(frame) = frame else {
        return;
    };
    base_rows(frame, plane, 64, HEIGHT as i32, 0, WIDTH / 8);
    draw_frosted_system_header(bar, plane, Some(frame));
}
pub fn draw_canonical_wordmark(frame: Option<&mut Frame>, plane: HomePlane) {
    let Some(frame) = frame else {
        return;
    };
    for y in 12..40 {
        for x in 28..114 {
            pixel(frame, plane, x, y, base_tone(x, y));
        }
    }
}
pub fn draw_system_footer(frame: Option<&mut Frame>, buttons: SystemButtons<'_>, plane: HomePlane) {
    let Some(frame) = frame else {
        return;
    };
    frame[748 * (WIDTH / 8)..].fill(if plane == HomePlane::Monochrome {
        0
    } else {
        255
    });
    for (x, label) in [95, 201, 331, 437].into_iter().zip([
        buttons.back,
        buttons.confirm,
        buttons.previous,
        buttons.next,
    ]) {
        if cstr(label).is_empty() {
            continue;
        }
        base_rows(
            frame,
            plane,
            748,
            HEIGHT as i32,
            (x - 49) as usize / 8,
            (x + 49) as usize / 8 + 1,
        );
        home_text(frame, plane, x, 779, label, Face::R28, 94, 1);
    }
}
pub fn system_footer_shows(frame: Option<&Frame>, slot: usize) -> bool {
    let (Some(frame), Some(&x)) = (frame, [95usize, 201, 331, 437].get(slot)) else {
        return false;
    };
    (748..HEIGHT)
        .any(|y| ((x - 49) / 8..=(x + 49) / 8).any(|byte| frame[y * (WIDTH / 8) + byte] != 0))
}
/// Disjoint borrows forbid aliasing `source` and `frame`. Absent source/dest
/// and unsupported geometries retain the destination. Scratch: one landscape
/// row, frame::HEIGHT bytes.
pub fn frost_sleep_backdrop(
    source: Option<&Frame>,
    frame: Option<&mut Frame>,
    geometry: Geometry,
    plane: HomePlane,
) {
    let (Some(source), Some(frame)) = (source, frame) else {
        return;
    };
    let (w, h) = (i32::from(geometry.width), i32::from(geometry.height));
    if !newt_core::frame::is_frame_size(w as usize, h as usize) {
        return;
    }
    let radius = 12;
    let mut columns = [0u8; newt_core::frame::HEIGHT];
    let ink = |x: i32, y: i32| {
        let at = (y * w + x) as usize;
        (source[at / 8] >> (7 - (at & 7))) & 1
    };
    for yy in 0..=radius {
        for x in 0..w {
            columns[x as usize] += ink(x, yy);
        }
    }
    const BAYER: [u32; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
    for y in 0..h {
        if y > 0 {
            for x in 0..w {
                if y > radius {
                    columns[x as usize] -= ink(x, y - radius - 1);
                }
                if y + radius < h {
                    columns[x as usize] += ink(x, y + radius);
                }
            }
        }
        let rows = ((h - 1).min(y + radius) - 0.max(y - radius) + 1) as u32;
        let mut black = 0u32;
        for xx in 0..=radius {
            black += u32::from(columns[xx as usize]);
        }
        for x in 0..w {
            if x > 0 {
                if x > radius {
                    black -= u32::from(columns[(x - radius - 1) as usize]);
                }
                if x + radius < w {
                    black += u32::from(columns[(x + radius) as usize]);
                }
            }
            let area = rows * ((w - 1).min(x + radius) - 0.max(x - radius) + 1) as u32;
            let denominator = 255 * area;
            let light = (denominator - black * 128) * 3;
            let threshold = BAYER[((y & 3) * 4 + (x & 3)) as usize];
            let mut tone = light / denominator;
            if tone < 3 && (light % denominator) * 16 > denominator * threshold {
                tone += 1;
            }
            let set = match plane {
                HomePlane::Monochrome => black * 128 * 16 > denominator * threshold,
                HomePlane::Lsb => tone & 1 != 0,
                HomePlane::Msb => tone & 2 != 0,
            };
            let at = (y * w + x) as usize;
            let mask = 0x80 >> (at & 7);
            if set {
                frame[at / 8] |= mask;
            } else {
                frame[at / 8] &= !mask;
            }
        }
    }
}
