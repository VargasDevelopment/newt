// Pinned layout-private decoder intentionally differs from strict XML UTF-8.
#![forbid(unsafe_code)]
pub fn decode(t: &[u8], i: &mut usize) -> u32 {
    let b = t[*i];
    let (mut cp, mut n) = if b >= 0xf0 {
        (u32::from(b & 7), 3)
    } else if b >= 0xe0 {
        (u32::from(b & 15), 2)
    } else if b >= 0xc0 {
        (u32::from(b & 31), 1)
    } else {
        (u32::from(b), 0)
    };
    *i += 1;
    while n > 0 && *i < t.len() && t[*i] & 0xc0 == 0x80 {
        cp = (cp << 6) | u32::from(t[*i] & 63);
        *i += 1;
        n -= 1
    }
    cp
}
pub fn encode(cp: u32, out: &mut [u8]) -> usize {
    if cp < 0x80 {
        out[0] = cp as u8;
        1
    } else if cp < 0x800 {
        out[0] = 0xc0 | (cp >> 6) as u8;
        out[1] = 0x80 | (cp & 63) as u8;
        2
    } else if cp < 0x10000 {
        out[0] = 0xe0 | (cp >> 12) as u8;
        out[1] = 0x80 | ((cp >> 6) & 63) as u8;
        out[2] = 0x80 | (cp & 63) as u8;
        3
    } else {
        out[0] = 0xf0 | (cp >> 18) as u8;
        out[1] = 0x80 | ((cp >> 12) & 63) as u8;
        out[2] = 0x80 | ((cp >> 6) & 63) as u8;
        out[3] = 0x80 | (cp & 63) as u8;
        4
    }
}
pub fn count(t: &[u8]) -> u32 {
    t.iter().filter(|b| **b & 0xc0 != 0x80).count() as u32
}
pub fn local(n: &[u8]) -> &[u8] {
    &n[n.iter().rposition(|b| *b == b':').map_or(0, |i| i + 1)..]
}
pub fn hyph(cp: u32) -> bool {
    matches!(cp,65..=90|97..=122|0xc0..=0x24f|0x370..=0x3ff|0x400..=0x4ff)
        && cp != 0xd7
        && cp != 0xf7
}
pub fn cjk(cp: u32) -> bool {
    matches!(cp,0x2e80..=0x9fff|0xac00..=0xd7af|0xf900..=0xfaff|0xff00..=0xff60|0x20000..=0x3ffff)
}
pub fn hangul(cp: u32) -> bool {
    matches!(cp,0xac00..=0xd7af|0x1100..=0x11ff|0x3130..=0x318f|0xd7b0..=0xd7ff)
}
pub fn latin(cp: u32) -> bool {
    matches!(cp,48..=57|65..=90|97..=122)
}
pub fn crosses(a: u32, b: u32) -> bool {
    (cjk(a) && !hangul(a) && latin(b)) || (latin(a) && cjk(b) && !hangul(b))
}
fn trailing(cp: u32) -> bool {
    matches!(
        cp,
        0x3001
            | 0x3002
            | 0xff0c
            | 0xff0e
            | 0xff1a
            | 0xff1b
            | 0x3009
            | 0x300b
            | 0x300d
            | 0x300f
            | 0x3011
            | 0x3015
            | 0x3017
            | 0x3019
            | 0x301b
            | 0xff09
            | 0xff3d
            | 0xff5d
    )
}
fn leading(cp: u32) -> bool {
    matches!(
        cp,
        0x3008
            | 0x300a
            | 0x300c
            | 0x300e
            | 0x3010
            | 0x3014
            | 0x3016
            | 0x3018
            | 0x301a
            | 0xff08
            | 0xff3b
            | 0xff5b
    )
}
pub fn compression(a: u32, b: u32, size: u16) -> i32 {
    if (trailing(a) || leading(a)) && (trailing(b) || leading(b)) && (trailing(a) || leading(b)) {
        -(i32::from(size) / 2)
    } else {
        0
    }
}
fn class(cp: u32) -> u8 {
    if matches!(cp,0x590..=0x5ff|0xfb1d..=0xfb4f|0x600..=0x7bf|0x8a0..=0x8ff|0xfb50..=0xfdff|0xfe70..=0xfeff)
    {
        1
    } else if matches!(cp, 48..=57) {
        2
    } else if matches!(cp,65..=90|97..=122|0xc0..=0x24f|0x370..=0x58f|0x2e80..=u32::MAX) {
        0
    } else {
        3
    }
}
pub fn bidi(t: &[u8], levels: &mut [u8]) -> bool {
    bidi_with_arabic(t, levels).0
}

// Resolve neutral runs as they close; each output byte is written once.
pub(crate) fn bidi_with_arabic(t: &[u8], levels: &mut [u8]) -> (bool, bool) {
    let (mut i, mut base) = (0, 0);
    while i < t.len() {
        match class(decode(t, &mut i)) {
            1 => { base = 1; break; }
            0 => break,
            _ => {}
        }
    }
    let mut has_arabic = false;
    let mut previous = base;
    let mut pending = None;
    i = 0;
    while i < t.len() {
        let start = i;
        let cp = decode(t, &mut i);
        has_arabic |= (0x600..=0x77f).contains(&cp);
        let level = match class(cp) {
            1 => 1,
            0 | 2 => if base == 0 { 0 } else { 2 },
            _ => {
                if pending.is_none() { pending = Some(start); }
                continue;
            }
        };
        if let Some(from) = pending.take() {
            levels[from..start].fill(if previous == level { previous } else { base });
        }
        levels[start..i].fill(level);
        previous = level;
    }
    if let Some(from) = pending { levels[from..t.len()].fill(base); }
    (base == 1, has_arabic)
}
pub fn mirror(cp: u32) -> u32 {
    match cp {
        40 => 41,
        41 => 40,
        91 => 93,
        93 => 91,
        123 => 125,
        125 => 123,
        60 => 62,
        62 => 60,
        0xab => 0xbb,
        0xbb => 0xab,
        _ => cp,
    }
}
pub fn reverse(t: &mut [u8]) {
    t.reverse();
    let mut i = 0;
    while i < t.len() {
        let start = i;
        while i < t.len() && t[i] & 0xc0 == 0x80 {
            i += 1
        }
        if i < t.len() {
            i += 1
        }
        t[start..i].reverse();
    }
}
pub fn focus(t: &[u8], levels: &mut [u8]) {
    let mut i = 0;
    while i < t.len() {
        let start = i;
        let mut probe = i;
        let mut chars = 0;
        while probe < t.len() {
            let mut n = probe;
            let cp = decode(t, &mut n);
            if !hyph(cp) && !matches!(cp, 39 | 0x2018 | 0x2019) {
                break;
            }
            probe = n;
            chars += 1
        }
        if chars == 0 {
            decode(t, &mut i);
            continue;
        }
        let mut j = start;
        for _ in 0..(chars * 45 / 100).clamp(1, 9) {
            let at = j;
            decode(t, &mut j);
            for l in &mut levels[at..j] {
                *l |= 64
            }
        }
        i = probe
    }
}
