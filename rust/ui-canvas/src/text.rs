use crate::{atlas, cstr};
use newt_core::utf8;
const LATIN: &[u8] = concat!(
    "AAAAAA*CEEEEIIIIDNOOOOOxOUUUUY**",
    "aaaaaa*ceeeeiiiidnooooo*ouuuuy*y",
    "AaAaAaCcCcCcCcDdDdEeEeEeEeEeGgGg",
    "GgGgHhHhIiIiIiIiIi**JjKkkLlLlLlL",
    "lLlNnNnNn*NnOoOoOo**RrRrRrSsSsSs",
    "SsTtTtTtUuUuUuUuUuUuWwYyYZzZzZzs"
)
.as_bytes();
const ROMAN: [&[u8]; 16] = [
    b"I", b"II", b"III", b"IV", b"V", b"VI", b"VII", b"VIII", b"IX", b"X", b"XI", b"XII", b"L",
    b"C", b"D", b"M",
];
fn spelling(cp: u32, out: &mut [u8; 5]) -> Option<usize> {
    out.fill(0);
    let mut one = None;
    let bytes: &[u8] = match cp {
        0x20..=0x7e => {
            one = Some(cp as u8);
            b""
        }
        0xb7 | 0x2022 => "·".as_bytes(),
        0x2019 => "’".as_bytes(),
        0x2026 => "…".as_bytes(),
        9 | 10 | 13 | 0xa0 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 => b" ",
        0..=0x1f
        | 0x7f..=0x9f
        | 0xad
        | 0x300..=0x36f
        | 0x200b..=0x200f
        | 0x202a..=0x202e
        | 0x2060..=0x206f
        | 0xfe00..=0xfe0f
        | 0xfeff
        | 0xa1
        | 0xbf => b"",
        0x2010..=0x2015 | 0x2212 => b"-",
        0x2018 | 0x201a | 0x201b | 0x2032 | 0x2039 | 0x203a => b"'",
        0x201c..=0x201f | 0x2033 | 0xab | 0xbb => b"\"",
        0x2024 => b".",
        0x2025 => b"..",
        0x2044 => b"/",
        0xaa => b"a",
        0xba => b"o",
        0xb9 => b"1",
        0xb2 => b"2",
        0xb3 => b"3",
        0xbc => b"1/4",
        0xbd => b"1/2",
        0xbe => b"3/4",
        0xc0..=0x17f => match cp {
            0xc6 => b"AE",
            0xde => b"Th",
            0xdf => b"ss",
            0xe6 => b"ae",
            0xfe => b"th",
            0x132 => b"IJ",
            0x133 => b"ij",
            0x149 => "’n".as_bytes(),
            0x152 => b"OE",
            0x153 => b"oe",
            _ => {
                let c = LATIN[(cp - 0xc0) as usize];
                if c == b'*' {
                    return None;
                }
                one = Some(c);
                b""
            }
        },
        0x218..=0x21b => {
            one = Some(b"SsTt"[(cp - 0x218) as usize]);
            b""
        }
        0x2160..=0x217f => ROMAN[(cp & 15) as usize],
        0xfb00..=0xfb06 => {
            [&b"ff"[..], b"fi", b"fl", b"ffi", b"ffl", b"st", b"st"][(cp - 0xfb00) as usize]
        }
        0xff01..=0xff5e => {
            one = Some((cp - 0xfee0) as u8);
            b""
        }
        _ => return None,
    };
    if let Some(b) = one {
        out[0] = b;
        return Some(1);
    }
    out[..bytes.len()].copy_from_slice(bytes);
    if (0x2170..=0x217f).contains(&cp) {
        out[..bytes.len()].make_ascii_lowercase();
    }
    Some(bytes.len())
}
/// Empty output represents absent or zero-capacity storage. All nonempty outputs
/// are NUL terminated; bytes beyond that terminator retain their old values.
pub fn fold(text: Option<&[u8]>, out: &mut [u8], drop: bool) -> bool {
    if out.is_empty() {
        return false;
    }
    out[0] = 0;
    if out.len() < 4 {
        return false;
    }
    let text = cstr(text);
    let mut at = 0;
    let mut used = 0;
    let mut safe = 0;
    let mut full = false;
    let mut gap = false;
    let mut dropped = false;
    let mut named = false;
    while at < text.len() {
        let mut cp = 0;
        let mut spelled = [0; 5];
        let n = if utf8::next(Some(text), &mut at, &mut cp) {
            spelling(cp, &mut spelled)
        } else {
            None
        };
        let Some(n) = n else {
            if !drop {
                out[0] = 0;
                return false;
            }
            dropped = true;
            gap = used != 0;
            continue;
        };
        named |= spelled[..n].iter().any(u8::is_ascii_alphanumeric);
        if full || n == 0 {
            continue;
        }
        if spelled[0] == b' ' {
            gap = used != 0;
            continue;
        }
        if used + usize::from(gap) + n + 1 > out.len() {
            full = true;
            out[safe..safe + 3].copy_from_slice("…".as_bytes());
            used = safe + 3;
            continue;
        }
        if gap {
            out[used] = b' ';
            used += 1;
        }
        gap = false;
        out[used..used + n].copy_from_slice(&spelled[..n]);
        used += n;
        if used + 4 <= out.len() {
            safe = used;
        }
    }
    out[used] = 0;
    if dropped && !named {
        out[0] = 0;
        return false;
    }
    used != 0
}
pub fn fold_text(text: Option<&[u8]>, out: &mut [u8]) -> bool {
    fold(text, out, false)
}
pub(crate) fn copy_cstr(input: &[u8], out: &mut [u8]) {
    if out.is_empty() {
        return;
    }
    let input = cstr(Some(input));
    let n = input.len().min(out.len() - 1);
    out[..n].copy_from_slice(&input[..n]);
    out[n] = 0;
}
pub fn folded_name(name: Option<&[u8]>, fallback: &[u8], out: &mut [u8]) {
    if !fold(name, out, true) {
        copy_cstr(fallback, out);
    }
}
pub fn book_title(title: Option<&[u8]>, out: &mut [u8]) {
    folded_name(title, b"Your SD book", out);
}
/// Mirrors the pinned host's unsigned-long (64-bit) Section numbering. RV32's
/// unsigned-long overflow at u32::MAX is deliberately exposed separately.
pub fn chapter_label(label: Option<&[u8]>, spine: u32, out: &mut [u8]) {
    chapter_label_ulong(label, spine, out, cfg!(target_pointer_width = "32"));
}
pub fn chapter_label_ulong(label: Option<&[u8]>, spine: u32, out: &mut [u8], rv32: bool) {
    if fold(label, out, false) {
        return;
    }
    let mut buf = [0; 32];
    buf[..8].copy_from_slice(b"Section ");
    let mut n = if rv32 {
        u64::from(spine.wrapping_add(1))
    } else {
        u64::from(spine) + 1
    };
    let mut digits = [0; 20];
    let mut at = 20;
    loop {
        at -= 1;
        digits[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let len = 20 - at;
    buf[8..8 + len].copy_from_slice(&digits[at..]);
    copy_cstr(&buf[..8 + len], out);
}
pub fn ellipsize(text: &mut [u8], pixels: u32, width: i32) {
    if atlas::text_width(Some(text), pixels, false) <= width {
        return;
    }
    let p = pixels.clamp(8, 40);
    let face = atlas::face(p, false);
    let len = cstr(Some(text)).len();
    let mut cursor = 0;
    let mut previous = 0;
    let mut pen = 0;
    let mut keep = 0;
    while cursor < len {
        if cursor + 4 > text.len()
            || atlas::pixels(
                pen + atlas::kern(face, previous, 8230)
                    + i32::from(atlas::glyph(face, 8230, 0).advance),
                face,
                p,
            ) > width
        {
            break;
        }
        keep = cursor;
        let cp = atlas::next(&text[..len], &mut cursor);
        pen += atlas::kern(face, previous, cp) + i32::from(atlas::glyph(face, cp, 0).advance);
        previous = cp;
    }
    while keep != 0 && b" ,:;-".contains(&text[keep - 1]) {
        keep -= 1;
    }
    if text.len() >= 4 {
        text[keep..keep + 4].copy_from_slice("…\0".as_bytes());
    } else if !text.is_empty() {
        text[0] = 0;
    }
}
/// Byte-at-a-time measuring, 251-byte rows and 2048-byte input limit are
/// intentional compatibility constraints, including the last-space fallback.
pub(crate) fn rows(
    value: Option<&[u8]>,
    pixels: u32,
    width: i32,
    word_wrap: bool,
    mut accept: impl FnMut(&[u8]) -> bool,
) -> bool {
    let v = cstr(value);
    let mut pos = 0;
    while pos < v.len() && pos < 2048 {
        while pos < 2048 && v.get(pos) == Some(&b' ') {
            pos += 1;
        }
        if pos == v.len() {
            return true;
        }
        let mut row = [0; 256];
        let mut count = 0;
        let mut last_space = 0;
        while pos + count < v.len() && pos + count < 2048 && count < 251 {
            row[count] = v[pos + count];
            count += 1;
            row[count] = 0;
            if row[count - 1] == b' ' {
                last_space = count - 1;
            }
            if atlas::text_width(Some(&row), pixels, false) > width {
                count -= 1;
                break;
            }
        }
        if count == 0 {
            return false;
        }
        if word_wrap && pos + count < v.len() && last_space != 0 {
            count = last_space;
        }
        while count != 0 && v.get(pos + count).is_some_and(|b| b & 0xc0 == 0x80) {
            count -= 1;
        }
        if count == 0 {
            return false;
        }
        row[count] = 0;
        if !accept(&row[..count + 1]) {
            return false;
        }
        pos += count;
    }
    pos == v.len()
}
pub fn paragraph_rows(value: Option<&[u8]>, pixels: u32, width: i32) -> i32 {
    let mut n = 0;
    if rows(value, pixels, width, true, |_| {
        n += 1;
        true
    }) {
        n
    } else {
        -1
    }
}
