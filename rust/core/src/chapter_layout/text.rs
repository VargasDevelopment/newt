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
fn class(cp: u32, case_strong_l: bool) -> u8 {
    if matches!(cp,0x590..=0x5ff|0xfb1d..=0xfb4f|0x600..=0x7bf|0x8a0..=0x8ff|0xfb50..=0xfdff|0xfe70..=0xfeff)
    {
        1
    } else if matches!(cp, 48..=57) {
        2
    } else if matches!(cp,65..=90|97..=122|0xc0..=0x24f|0x370..=0x58f|0x2e80..=u32::MAX)
        || (case_strong_l
            && matches!(cp, 0x00b5 | 0x1e00..=0x1eff | 0x2126 | 0x212a..=0x212b | 0x2132 | 0x214e))
    {
        0
    } else {
        3
    }
}
pub(crate) const DIR_NONE: u8 = 0;
const DIR_BASE_MASK: u8 = 0x03;
const DIR_LTR: u8 = 1;
const DIR_RTL: u8 = 2;
const DIR_PREV_SET: u8 = 0x04;
const DIR_PREV_SHIFT: u8 = 3;
const DIR_PREV_MASK: u8 = 0x03 << DIR_PREV_SHIFT;
pub(crate) const DIR_FOCUS_WORD: u8 = 0x20;

pub fn bidi(t: &[u8], levels: &mut [u8]) -> bool {
    let mut dir = DIR_NONE;
    bidi_with_arabic(t, levels, false, &mut dir, false).0
}

// Resolve neutral runs as they close; each output byte is written once.
pub(crate) fn bidi_with_arabic(
    t: &[u8],
    levels: &mut [u8],
    case_strong_l: bool,
    par_dir: &mut u8,
    lock_dir: bool,
) -> (bool, bool) {
    let base_bits = *par_dir & DIR_BASE_MASK;
    let base = if lock_dir && base_bits != DIR_NONE {
        u8::from(base_bits == DIR_RTL)
    } else {
        let (mut i, mut found) = (0, DIR_NONE);
        while i < t.len() {
            match class(decode(t, &mut i), case_strong_l) {
                1 => { found = DIR_RTL; break; }
                0 => { found = DIR_LTR; break; }
                _ => {}
            }
        }
        if base_bits == DIR_NONE && found != DIR_NONE {
            *par_dir = (*par_dir & !DIR_BASE_MASK) | found;
        }
        u8::from(found == DIR_RTL)
    };
    let mut has_arabic = false;
    let mut previous = if lock_dir && *par_dir & DIR_PREV_SET != 0 {
        (*par_dir & DIR_PREV_MASK) >> DIR_PREV_SHIFT
    } else {
        base
    };
    let mut pending = None;
    let mut i = 0;
    while i < t.len() {
        let start = i;
        let cp = decode(t, &mut i);
        has_arabic |= (0x600..=0x77f).contains(&cp);
        let level = match class(cp, case_strong_l) {
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
    if *par_dir & DIR_BASE_MASK != DIR_NONE {
        *par_dir = (*par_dir & !(DIR_PREV_MASK | DIR_PREV_SET))
            | DIR_PREV_SET
            | ((previous & 0x03) << DIR_PREV_SHIFT);
    }
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
pub(crate) fn is_focus_word(cp: u32) -> bool {
    hyph(cp) || matches!(cp, 39 | 0x2018 | 0x2019)
}

pub(crate) fn utf8_incomplete(bytes: &[u8]) -> bool {
    let Some(&lead) = bytes.first() else {
        return false;
    };
    let need = if (0xf0..=0xf4).contains(&lead) {
        4
    } else if (0xe0..=0xef).contains(&lead) {
        3
    } else if (0xc2..=0xdf).contains(&lead) {
        2
    } else {
        1
    };
    bytes.len() < need && bytes[1..].iter().all(|&b| b & 0xc0 == 0x80)
}

fn prev_char(text: &[u8], end: usize) -> Option<(usize, u32)> {
    if end == 0 {
        return None;
    }
    let mut start = end - 1;
    while start > 0 && text[start] & 0xc0 == 0x80 {
        start -= 1;
    }
    let mut probe = start;
    let cp = decode(&text[..end], &mut probe);
    (probe == end).then_some((start, cp))
}

pub(crate) fn is_combining(cp: u32) -> bool {
    matches!(
        cp,
        0x0300..=0x036f
            | 0x0591..=0x05bd
            | 0x05bf
            | 0x05c1..=0x05c2
            | 0x05c4..=0x05c5
            | 0x05c7
            | 0x0610..=0x061a
            | 0x064b..=0x065f
            | 0x0670
            | 0x06d6..=0x06dc
            | 0x06df..=0x06e4
            | 0x06e7..=0x06e8
            | 0x06ea..=0x06ed
            | 0x1ab0..=0x1aff
            | 0x1dc0..=0x1dff
            | 0x20d0..=0x20ff
            | 0xfe20..=0xfe2f
    )
}

fn is_token_char(cp: u32) -> bool {
    !matches!(cp, 32 | 10) && !cjk(cp)
}

pub(crate) fn continuation_rewind(
    text: &[u8],
    mut next_cp: Option<u32>,
    case_strong_l: bool,
    par_dir: u8,
    min_cut: usize,
) -> usize {
    let end = text.len();
    if end == 0 || min_cut >= end || next_cp == Some(10) {
        return end;
    }
    let is_word = |cp: u32| {
        is_focus_word(cp) || is_combining(cp) || (case_strong_l && super::case::is_word_char(cp))
    };
    let base_bits = par_dir & DIR_BASE_MASK;
    let mut pos = end;
    let mut need_token_head = false;
    loop {
        if pos <= min_cut {
            break;
        }
        let before_step = pos;
        // 1. Keep incomplete graphemes (base + combining marks), words, Arabic clusters,
        //    and attached non-space/non-CJK tokens intact across continuing flushes. Each
        //    wider layer commits only if its candidate stays `>= min_cut`, so an over-wide
        //    extension can never discard a narrower legal cut.
        let attach = need_token_head
            || next_cp.is_none_or(|cp| cp != 0xad && is_token_char(cp));
        if next_cp.is_none_or(is_combining) {
            let mut cand = pos;
            let mut found_grapheme = false;
            while let Some((ch_start, cp)) = prev_char(text, cand) {
                if is_combining(cp) {
                    cand = ch_start;
                    found_grapheme = true;
                } else if cp == 0x00ad {
                    cand = ch_start;
                } else {
                    if is_token_char(cp) {
                        cand = ch_start;
                        found_grapheme = true;
                    }
                    break;
                }
            }
            if found_grapheme && cand < pos && cand >= min_cut {
                pos = cand;
            }
        }
        let rewind_word = match next_cp {
            Some(cp) => is_word(cp),
            None => prev_char(text, pos).is_some_and(|(_, cp)| is_word(cp)),
        };
        if rewind_word {
            let mut cand = pos;
            while let Some((ch_start, cp)) = prev_char(text, cand) {
                if is_word(cp) {
                    cand = ch_start;
                } else {
                    break;
                }
            }
            if cand >= min_cut {
                pos = cand;
            }
        } else {
            let rewind_arabic = match next_cp {
                Some(cp) => super::arabic::join(cp) != 0,
                None => prev_char(text, pos)
                    .is_some_and(|(_, cp)| matches!(super::arabic::join(cp), 1 | 2 | 3 | 5)),
            };
            if rewind_arabic {
                // 2. Keep Arabic joining clusters and lam-alef lookahead intact.
                let mut cand = pos;
                while let Some((ch_start, cp)) = prev_char(text, cand) {
                    if matches!(super::arabic::join(cp), 1 | 2 | 3 | 5) {
                        cand = ch_start;
                    } else {
                        break;
                    }
                }
                if cand >= min_cut {
                    pos = cand;
                }
            }
        }
        // Preserve non-breaking attachment (e.g. punctuation, combining marks, or mid-token
        // splits) by extending back to the enclosing non-space/non-CJK token start whenever
        // a break opportunity exists at or past `min_cut`; otherwise retain the closer cut.
        if (pos < before_step || attach)
            && prev_char(text, pos).is_some_and(|(_, cp)| is_token_char(cp))
        {
            let mut ext = pos;
            let mut found_visible = false;
            while let Some((ch_start, cp)) = prev_char(text, ext) {
                if is_token_char(cp) {
                    ext = ch_start;
                    if cp != 0x00ad {
                        found_visible = true;
                    }
                } else {
                    break;
                }
            }
            if found_visible && ext >= min_cut {
                pos = ext;
            }
        }
        if pos < before_step {
            let mut probe = pos;
            next_cp = Some(decode(&text[..end], &mut probe));
        }
        // 3. Defer trailing BiDi neutrals immediately before `pos` only when their level
        //    can actually depend on the following chunk (`previous != base` or unknown base).
        let mut scan = pos;
        let mut neutral_cut = pos;
        let mut moved_cp = 0u32;
        while let Some((ch_start, cp)) = prev_char(text, scan) {
            if class(cp, case_strong_l) == 3 && cp != 10 {
                scan = ch_start;
                if cp != 32 && cp != 0xad {
                    neutral_cut = scan;
                    moved_cp = cp;
                }
            } else {
                break;
            }
        }
        if neutral_cut < pos && neutral_cut >= min_cut {
            let base: Option<u8> = if base_bits != DIR_NONE {
                Some(u8::from(base_bits == DIR_RTL))
            } else {
                let mut i = 0;
                let mut found = None;
                while i < scan {
                    match class(decode(&text[..scan], &mut i), case_strong_l) {
                        1 => {
                            found = Some(1);
                            break;
                        }
                        0 => {
                            found = Some(0);
                            break;
                        }
                        _ => {}
                    }
                }
                found
            };
            let mut prev_scan = scan;
            let mut prev_level: Option<u8> = None;
            while let Some((ch_start, cp)) = prev_char(text, prev_scan) {
                match class(cp, case_strong_l) {
                    1 => {
                        prev_level = Some(1);
                        break;
                    }
                    0 | 2 => {
                        prev_level = Some(if base == Some(1) { 2 } else { 0 });
                        break;
                    }
                    _ => {
                        prev_scan = ch_start;
                    }
                }
            }
            let effective_prev = prev_level.or_else(|| {
                if par_dir & DIR_PREV_SET != 0 {
                    Some((par_dir & DIR_PREV_MASK) >> DIR_PREV_SHIFT)
                } else {
                    base
                }
            });
            let next_level = next_cp.and_then(|cp| match class(cp, case_strong_l) {
                1 => Some(1),
                0 | 2 => base.map(|b| if b == 1 { 2 } else { 0 }),
                _ => None,
            });
            let unresolved = match (base, effective_prev) {
                (None, _) => true,
                (Some(b), Some(p)) => p != b && next_level.is_none_or(|nl| nl == p),
                (Some(_), None) => false,
            };
            if unresolved {
                pos = neutral_cut;
                next_cp = Some(moved_cp);
                need_token_head = true;
                continue;
            }
        }
        break;
    }
    pos
}

pub fn focus(t: &[u8], levels: &mut [u8]) {
    focus_with_carry(t, levels, false);
}

pub(crate) fn focus_with_carry(t: &[u8], levels: &mut [u8], skip_leading_word: bool) -> bool {
    let mut i = 0;
    if skip_leading_word {
        while i < t.len() {
            let mut n = i;
            if !is_focus_word(decode(t, &mut n)) {
                break;
            }
            i = n;
        }
        if i == t.len() {
            return true;
        }
    }
    let mut ends_in_word = false;
    while i < t.len() {
        let start = i;
        let mut probe = i;
        let mut chars = 0;
        while probe < t.len() {
            let mut n = probe;
            let cp = decode(t, &mut n);
            if !is_focus_word(cp) {
                break;
            }
            probe = n;
            chars += 1
        }
        if chars == 0 {
            decode(t, &mut i);
            ends_in_word = false;
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
        i = probe;
        ends_in_word = i == t.len();
    }
    ends_in_word
}

#[cfg(test)]
mod rewind_tests {
    use super::*;

    #[test]
    fn dangling_soft_hyphen_after_delimiter_is_not_carried_while_real_tokens_are() {
        // R32-1: Dangling soft hyphen(s) after space, newline, or CJK must not be
        // rewound by either the grapheme scan or the whole-token fallback.
        for (sample, next) in [
            ("a \u{00ad}", Some(0x0301)),
            ("a\n\u{00ad}", Some(0x0301)),
            ("\u{6f22}\u{00ad}", Some(0x0301)),
            ("a \u{00ad}\u{00ad}", Some(b'b' as u32)),
            ("a \u{00ad}", None),
        ] {
            let bytes = sample.as_bytes();
            assert_eq!(
                continuation_rewind(bytes, next, true, DIR_NONE, 1),
                bytes.len(),
                "dangling marker rewound for {sample:?} next={next:?}"
            );
        }
        // Real tokens with interior or trailing soft hyphens still carry intact.
        assert_eq!(continuation_rewind("a dog\u{00ad}s".as_bytes(), Some(b'.' as u32), true, DIR_NONE, 1), 2);
        assert_eq!(continuation_rewind("a dogs\u{00ad}".as_bytes(), Some(0x0301), true, DIR_NONE, 1), 2);
    }
}
